//! Nova — the heavy-weapons formation shooter.
//!
//! Where `galaga` is the plain arcade original, Nova is a full shoot-'em-up
//! campaign on the same court: you build a ship, fly sectors that each fight
//! differently, level a pilot up and spend salvage in a hangar between waves.
//!
//! * **Hulls** — Interceptor (fast, fragile, blinks), Cruiser (balanced, raises
//!   a bulwark), Juggernaut (armoured, lays a barrage). Every hull carries a
//!   special paid for out of an energy meter.
//! * **Ship building** — five components (engine, reactor, plating, cannon,
//!   magazine) upgrade through four tiers, and five modules (magnet, autoloader,
//!   salvager, repair bay, overdrive) bolt on permanently. Everything is bought
//!   in the hangar between waves with salvage picked up from kills.
//! * **Guns** — ten of them, three levels each, with wing drones firing
//!   alongside: blaster, spread, piercing laser, homing missiles, wide plasma,
//!   the vulcan machine gun, dumb-fire rockets that blast a hole where they
//!   land, flak shells that burst into a fan, a rail slug that runs the whole
//!   court, and an arc bolt that earths itself through a crowd.
//! * **Progression** — kills pay experience and salvage; each pilot level hands
//!   out a permanent upgrade in a fixed rotation, and every extend threshold
//!   pays a spare hull.
//! * **Sectors** — open space, asteroid belt, nebula (drag on every shot),
//!   minefield, ion storm (surges that drain the reactor and stun drones) and
//!   debris ring (blocks that eat shots), each with its own backdrop.
//! * **Enemies** — grunts, weavers, aimed turrets, spread bombers, kamikazes,
//!   armoured tanks, telegraphing snipers, mine-laying miners, splitters that
//!   break in two, and healers that repair whatever flies beside them.
//! * **Bosses** — every fourth wave, cycling through a dreadnought, a twin whose
//!   core is armoured until both turrets fall, a carrier that launches minions
//!   from two bays, and a segmented serpent that swims behind its head. Each has
//!   three attack phases keyed to how much hull is left.
//!
//! Controls: `←/→`/`h`/`l` and `↑/↓`/`k`/`j` fly, `SPC` (or `f`) fires, `x`
//! triggers the hull special, `b` drops a smart bomb, `p` pauses, `r` retries
//! with the same build, `n` restarts, `q`/`Esc` quits. The picker takes `1`/`2`/
//! `3` or `←/→`, `d` cycles difficulty, `Enter` launches. In the hangar the
//! listed key buys the line and `Enter` launches the next wave.
//!
//! Like the other action games the overlay animates itself through
//! `zmax_event::request_redraw` while a round is live; all of the game state
//! below is pure and unit-tested and uses the same LCG PRNG as the snake port.

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Court width in cells.
const W: i16 = 76;
/// Court height in cells.
const H: i16 = 28;
/// Topmost row the ship may fly to; it owns the bottom seven rows.
const SHIP_TOP: i16 = H - 9;
/// The row the ship starts on.
const SHIP_ROW: i16 = H - 1;
/// Formation columns.
const COLS: usize = 12;
/// Formation rows the court has room for.
const ROWS: usize = 6;
/// Horizontal spacing between formation columns.
const ENEMY_GAP: i16 = 6;
/// Column of the leftmost formation column at zero sway.
const BASE_X: i16 = 5;
/// Row of the top formation row.
const FORMATION_TOP: i16 = 2;
/// Ticks between successive formation sway steps.
const SWAY_CADENCE: u32 = 5;
/// How far the formation drifts to either side.
const SWAY_MAX: i16 = 3;
/// 1-in-N chance per tick that a weaver breaks formation into its snaking run.
const WEAVE_CHANCE: u64 = 90;
/// Ticks of invulnerability granted after taking a hit.
const INVULN_TICKS: u32 = 24;
/// Ticks a kill chain survives without another kill.
const COMBO_TICKS: u32 = 60;
/// Ticks a rapid-fire pickup lasts.
const RAPID_TICKS: u32 = 240;
/// Highest kill-chain multiplier.
const MAX_COMBO: u32 = 8;
/// Highest level any gun can be upgraded to.
const MAX_WEAPON_LEVEL: u32 = 3;
/// Rows a flak shell climbs before it bursts.
const FLAK_FUSE: i16 = 6;
/// How far an arc bolt will reach for its next hull.
const ARC_REACH: i16 = 10;
/// Cap on player shots in flight.
const MAX_SHOTS: usize = 48;
/// Damage a smart bomb deals to everything on the court.
const BOMB_DAMAGE: i32 = 4;
/// Ticks between a cleared wave and the hangar opening.
const INTERMISSION_TICKS: u32 = 20;
/// Base 1-in-N chance a kill drops a powerup, before difficulty.
const DROP_CHANCE: u64 = 4;
/// Every Nth wave is a boss wave.
const BOSS_EVERY: u32 = 4;
/// Base energy meter, and what a hull special costs out of it.
const BASE_ENERGY: u32 = 100;
const SPECIAL_COST: u32 = 60;
/// Energy the meter recovers per tick before any reactor upgrade.
const ENERGY_REGEN: u32 = 1;
/// Columns an Interceptor blink covers, and the invulnerability it lands with.
const BLINK_DISTANCE: i16 = 10;
const BLINK_IFRAMES: u32 = 20;
/// Ticks a Cruiser bulwark holds enemy fire off the hull.
const BULWARK_TICKS: u32 = 60;
/// Columns between the bolts a Juggernaut barrage lays across the court.
const BARRAGE_STEP: i16 = 7;
/// The most shield pips any hull can ever carry.
const MAX_SHIELD_PIPS: u32 = 8;
/// Wing drones the ship can carry, and how far out they ride.
const MAX_DRONES: usize = 2;
const DRONE_OFFSET: i16 = 4;
/// A spare life every this many points.
const EXTEND_SCORE: u32 = 25_000;
/// Ticks a sniper spends telegraphing before its shot goes off.
const SNIPER_CHARGE: u32 = 8;
/// Ticks a mine sits before it goes off on its own, and how close the ship has
/// to fly to set it off early.
const MINE_FUSE: u32 = 200;
const MINE_TRIGGER: i16 = 2;
/// Hit points an asteroid carries, and the base 1-in-N odds one drifts in.
const ASTEROID_HP: i32 = 5;
const ASTEROID_CHANCE: u64 = 260;
/// Hit points a debris block carries.
const DEBRIS_HP: i32 = 9;
/// Ticks between the repairs a healer hands out, and how far its reach is.
const HEAL_CADENCE: u32 = 40;
const HEAL_RANGE: i16 = 6;
/// Points a medal pickup pays, and the salvage that comes with it.
const MEDAL_SCORE: u32 = 500;
/// Experience the next pilot level costs, multiplied by the level reached.
const XP_PER_LEVEL: u32 = 200;
/// Highest tier any ship component upgrades to.
const MAX_TIER: u32 = 4;
/// Ticks between the repair bay module handing back a shield pip.
const REPAIR_CADENCE: u32 = 300;
/// Energy an ion surge drains, and how long it stuns the drones for.
const SURGE_DRAIN: u32 = 25;
const SURGE_STUN: u32 = 40;
/// Rows between the bulkheads in a gates map, and the gap left in each.
const GATE_PERIOD: u32 = 7;
const GATE_GAP: i16 = 16;
/// Rows in one maze block, and how many of them are walled.
const MAZE_PERIOD: u32 = 8;
const MAZE_WALL: u32 = 4;
/// Rock columns a row may grow.
const PILLARS_PER_ROW: usize = 3;
/// Ticks between the rock scrolling one row down the court.
const SCROLL_CADENCE: u32 = 5;
/// Wall turrets a map keeps bolted to its rock, and what they carry.
const TURRETS_PER_MAP: usize = 2;
const TURRET_HP: i32 = 4;
const TURRET_CADENCE: u32 = 34;
/// Ticks between a solar flare stepping one column across the court, and the
/// rhythm it burns on: it is only hot for FLARE_ACTIVE ticks in FLARE_PERIOD.
const FLARE_CADENCE: u32 = 4;
const FLARE_PERIOD: u32 = 60;
const FLARE_ACTIVE: u32 = 18;
/// Stops the star chart offers between waves, and the keys that pick them.
const ROUTE_CHOICES: usize = 3;
const ROUTE_KEYS: [char; 3] = ['z', 'x', 'v'];
/// Ticks the arrival banner stays up for.
const BANNER_TICKS: u32 = 40;
/// Rows of parallax backdrop drawn behind the court.
const STAR_LAYERS: usize = 3;
/// Segments a serpent boss trails behind its head.
const SERPENT_SEGMENTS: usize = 8;
/// The vertical offsets a serpent's body cycles through as it swims.
const SERPENT_WAVE: [i16; 8] = [0, 1, 2, 1, 0, -1, -2, -1];
/// The keys the hangar hands out to its lines, in order.
const SHOP_KEYS: [char; 17] = [
    '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'c', 'e', 'g', 'i', 'm', 'o', 's',
];

/// How hard the run is, chosen on the picker.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Difficulty {
    Normal,
    Hard,
    Insane,
}

impl Difficulty {
    pub const ALL: [Difficulty; 3] = [Difficulty::Normal, Difficulty::Hard, Difficulty::Insane];

    pub fn name(self) -> &'static str {
        match self {
            Difficulty::Normal => "Normal",
            Difficulty::Hard => "Hard",
            Difficulty::Insane => "Insane",
        }
    }

    /// Extra hit points every enemy hull carries.
    pub fn armour(self) -> i32 {
        match self {
            Difficulty::Normal => 0,
            Difficulty::Hard => 1,
            Difficulty::Insane => 2,
        }
    }

    /// Divides the 1-in-N odds of enemies shooting, diving and spawning, so a
    /// harder run means a busier court.
    pub fn aggression(self) -> u64 {
        match self {
            Difficulty::Normal => 1,
            Difficulty::Hard => 2,
            Difficulty::Insane => 3,
        }
    }

    /// 1-in-N odds a kill drops something; drops thin out as it gets harder.
    pub fn drop_chance(self) -> u64 {
        DROP_CHANCE + self.aggression() - 1
    }

    /// Everything scored is multiplied by this.
    pub fn score_bonus(self) -> u32 {
        match self {
            Difficulty::Normal => 1,
            Difficulty::Hard => 2,
            Difficulty::Insane => 3,
        }
    }
}

/// The trick each hull carries, paid for with energy.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Special {
    /// Teleport several columns the way you were flying, landing invulnerable.
    Blink,
    /// A bubble that eats every shot that reaches the hull for a while.
    Bulwark,
    /// A wall of bolts laid across the whole court at once.
    Barrage,
}

impl Special {
    pub fn name(self) -> &'static str {
        match self {
            Special::Blink => "blink",
            Special::Bulwark => "bulwark",
            Special::Barrage => "barrage",
        }
    }
}

/// The three hulls, trading speed and rate of fire against armour and damage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShipClass {
    Interceptor,
    Cruiser,
    Juggernaut,
}

impl ShipClass {
    pub const ALL: [ShipClass; 3] = [
        ShipClass::Interceptor,
        ShipClass::Cruiser,
        ShipClass::Juggernaut,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ShipClass::Interceptor => "Interceptor",
            ShipClass::Cruiser => "Cruiser",
            ShipClass::Juggernaut => "Juggernaut",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            ShipClass::Interceptor => "▴",
            ShipClass::Cruiser => "▲",
            ShipClass::Juggernaut => "◭",
        }
    }

    /// Columns the hull slides per keypress before the engine is upgraded.
    pub fn speed(self) -> i16 {
        match self {
            ShipClass::Interceptor => 2,
            _ => 1,
        }
    }

    /// Shield pips it soaks before a hit costs a life.
    pub fn max_shield(self) -> u32 {
        match self {
            ShipClass::Interceptor => 1,
            ShipClass::Cruiser => 2,
            ShipClass::Juggernaut => 4,
        }
    }

    /// Ticks between shots before the magazine is upgraded.
    pub fn fire_cadence(self) -> u32 {
        match self {
            ShipClass::Interceptor => 2,
            ShipClass::Cruiser => 3,
            ShipClass::Juggernaut => 4,
        }
    }

    /// Damage each of its shots carries at gun level one.
    pub fn damage(self) -> i32 {
        match self {
            ShipClass::Interceptor => 1,
            ShipClass::Cruiser => 2,
            ShipClass::Juggernaut => 3,
        }
    }

    /// Smart bombs it launches with.
    pub fn bombs(self) -> u32 {
        match self {
            ShipClass::Interceptor => 2,
            ShipClass::Cruiser => 3,
            ShipClass::Juggernaut => 4,
        }
    }

    pub fn special(self) -> Special {
        match self {
            ShipClass::Interceptor => Special::Blink,
            ShipClass::Cruiser => Special::Bulwark,
            ShipClass::Juggernaut => Special::Barrage,
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            ShipClass::Interceptor => {
                "fast, fragile, fires twice as often; blinks across the court"
            }
            ShipClass::Cruiser => "the balanced hull; raises a bulwark that eats fire",
            ShipClass::Juggernaut => "slow and heavy, four shield pips; lays a barrage",
        }
    }
}

/// A component of the ship, upgraded tier by tier in the hangar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    /// Columns the hull covers per keypress.
    Engine,
    /// Energy capacity and regeneration.
    Reactor,
    /// Shield pips.
    Plating,
    /// Damage on every shot.
    Cannon,
    /// Ticks between shots.
    Magazine,
}

impl Part {
    pub const ALL: [Part; 5] = [
        Part::Engine,
        Part::Reactor,
        Part::Plating,
        Part::Cannon,
        Part::Magazine,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Part::Engine => "engine",
            Part::Reactor => "reactor",
            Part::Plating => "plating",
            Part::Cannon => "cannon",
            Part::Magazine => "magazine",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Part::Engine => "+1 column of thrust every two tiers",
            Part::Reactor => "+1 energy per tick and +10 capacity per tier",
            Part::Plating => "+1 shield pip per tier",
            Part::Cannon => "+1 damage per tier",
            Part::Magazine => "-1 tick between shots every two tiers",
        }
    }

    /// What the next tier costs; each one is dearer than the last.
    pub fn price(self, tier: u32) -> u32 {
        300 + 250 * tier
    }
}

/// A permanent bolt-on bought once.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Module {
    /// Pickups drift toward the ship.
    Magnet,
    /// One tick shaved off the firing cadence.
    Autoloader,
    /// Half again as much salvage from every kill.
    Salvager,
    /// A shield pip handed back every few seconds.
    RepairBay,
    /// The hull special costs a third less energy.
    Overdrive,
}

impl Module {
    pub const ALL: [Module; 5] = [
        Module::Magnet,
        Module::Autoloader,
        Module::Salvager,
        Module::RepairBay,
        Module::Overdrive,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Module::Magnet => "magnet",
            Module::Autoloader => "autoloader",
            Module::Salvager => "salvager",
            Module::RepairBay => "repair bay",
            Module::Overdrive => "overdrive",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Module::Magnet => "pickups drift toward the hull",
            Module::Autoloader => "-1 tick between shots",
            Module::Salvager => "+50% salvage from every kill",
            Module::RepairBay => "a shield pip back every 300 ticks",
            Module::Overdrive => "the special costs a third less energy",
        }
    }

    pub fn price(self) -> u32 {
        match self {
            Module::Salvager => 600,
            Module::Magnet => 700,
            Module::Overdrive => 800,
            Module::Autoloader => 900,
            Module::RepairBay => 1000,
        }
    }
}

/// Everything bolted to the ship: component tiers and the modules fitted.
#[derive(Clone, Debug, Default)]
pub struct Loadout {
    /// Tier of each component, indexed by `Part`.
    pub tiers: [u32; Part::ALL.len()],
    pub modules: Vec<Module>,
}

impl Loadout {
    pub fn tier(&self, part: Part) -> u32 {
        self.tiers[part as usize]
    }

    pub fn has(&self, module: Module) -> bool {
        self.modules.contains(&module)
    }

    fn upgrade(&mut self, part: Part) {
        self.tiers[part as usize] += 1;
    }
}

/// The five guns. Every one has three levels; a matching pickup upgrades the
/// gun you carry, a different one swaps it out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weapon {
    Blaster,
    Spread,
    Laser,
    Homing,
    Plasma,
    /// A stuttering machine gun: little rounds, almost no gap between them.
    Vulcan,
    /// Dumb-fire rockets that blow a hole where they land.
    Rocket,
    /// Shells that burst into a fan of fragments part way up the court.
    Flak,
    /// One slow, enormous piercing slug.
    Rail,
    /// A bolt that jumps from hull to hull.
    Arc,
}

impl Weapon {
    pub const ALL: [Weapon; 10] = [
        Weapon::Blaster,
        Weapon::Spread,
        Weapon::Laser,
        Weapon::Homing,
        Weapon::Plasma,
        Weapon::Vulcan,
        Weapon::Rocket,
        Weapon::Flak,
        Weapon::Rail,
        Weapon::Arc,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Weapon::Blaster => "blaster",
            Weapon::Spread => "spread",
            Weapon::Laser => "laser",
            Weapon::Homing => "homing",
            Weapon::Plasma => "plasma",
            Weapon::Vulcan => "vulcan",
            Weapon::Rocket => "rocket",
            Weapon::Flak => "flak",
            Weapon::Rail => "rail",
            Weapon::Arc => "arc",
        }
    }

    /// Ticks added to (or shaved off) the hull's firing cadence: a vulcan
    /// hoses, a rail gun takes its time.
    pub fn cadence_shift(self) -> i32 {
        match self {
            Weapon::Vulcan => -2,
            Weapon::Blaster | Weapon::Spread => 0,
            Weapon::Laser | Weapon::Arc => 1,
            Weapon::Homing | Weapon::Plasma | Weapon::Flak => 2,
            Weapon::Rocket => 3,
            Weapon::Rail => 6,
        }
    }

    /// The single letter its pickup shows on the court.
    pub fn tag(self) -> &'static str {
        match self {
            Weapon::Blaster => "B",
            Weapon::Spread => "S",
            Weapon::Laser => "L",
            Weapon::Homing => "H",
            Weapon::Plasma => "P",
            Weapon::Vulcan => "V",
            Weapon::Rocket => "R",
            Weapon::Flak => "F",
            Weapon::Rail => "X",
            Weapon::Arc => "A",
        }
    }

    /// The next gun in the rotation, for the hangar's swap.
    pub fn next(self) -> Weapon {
        let i = Weapon::ALL.iter().position(|&w| w == self).unwrap_or(0);
        Weapon::ALL[(i + 1) % Weapon::ALL.len()]
    }
}

/// What a shot looks like, and by extension how it reads on the court.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShotKind {
    Bolt,
    Beam,
    Missile,
    Plasma,
    Rocket,
    Flak,
    Rail,
    Arc,
    Enemy,
}

impl ShotKind {
    pub fn glyph(self) -> &'static str {
        match self {
            ShotKind::Bolt => "|",
            ShotKind::Beam => "┃",
            ShotKind::Missile => "↟",
            ShotKind::Plasma => "◍",
            ShotKind::Rocket => "⇈",
            ShotKind::Flak => "✱",
            ShotKind::Rail => "║",
            ShotKind::Arc => "≈",
            ShotKind::Enemy => "!",
        }
    }
}

/// One shot in flight, player or enemy. Player shots travel up (negative
/// `speed`), enemy shots down.
#[derive(Clone, Debug)]
pub struct Shot {
    pub pos: (i16, i16),
    /// Columns the shot slides per tick.
    pub drift: i16,
    /// Rows travelled per tick; negative is up the court.
    pub speed: i16,
    pub damage: i32,
    /// Beams keep travelling after they hit something.
    pub pierce: bool,
    /// Missiles re-aim at the nearest target every tick.
    pub homing: bool,
    /// Half-width of the damage footprint; plasma is three cells wide.
    pub half_width: i16,
    /// Blast radius on impact: a rocket takes the cells around what it hits.
    pub splash: i16,
    /// Rows left before a flak shell bursts; zero means it never does.
    pub fuse: i16,
    /// Hulls an arc bolt may still jump to.
    pub chain: u32,
    pub kind: ShotKind,
}

impl Shot {
    fn bolt(pos: (i16, i16), drift: i16, damage: i32) -> Shot {
        Shot {
            pos,
            drift,
            speed: -2,
            damage,
            pierce: false,
            homing: false,
            half_width: 0,
            splash: 0,
            fuse: 0,
            chain: 0,
            kind: ShotKind::Bolt,
        }
    }

    /// A machine-gun round: small, quick, no frills.
    fn vulcan(pos: (i16, i16), drift: i16, damage: i32) -> Shot {
        Shot {
            speed: -3,
            ..Shot::bolt(pos, drift, damage)
        }
    }

    /// A dumb-fire rocket: slow, heavy, and it takes the neighbours with it.
    fn rocket(pos: (i16, i16), damage: i32) -> Shot {
        Shot {
            speed: -1,
            splash: 1,
            kind: ShotKind::Rocket,
            ..Shot::bolt(pos, 0, damage)
        }
    }

    /// A flak shell: it climbs `fuse` rows, then bursts into fragments.
    fn flak(pos: (i16, i16), damage: i32, fuse: i16) -> Shot {
        Shot {
            speed: -2,
            fuse,
            kind: ShotKind::Flak,
            ..Shot::bolt(pos, 0, damage)
        }
    }

    /// A rail slug: the length of the court in a tick, through everything.
    fn rail(pos: (i16, i16), damage: i32) -> Shot {
        Shot {
            speed: -6,
            pierce: true,
            kind: ShotKind::Rail,
            ..Shot::bolt(pos, 0, damage)
        }
    }

    /// An arc bolt: it jumps to the next hull along, `chain` times.
    fn arc(pos: (i16, i16), damage: i32, chain: u32) -> Shot {
        Shot {
            speed: -2,
            chain,
            kind: ShotKind::Arc,
            ..Shot::bolt(pos, 0, damage)
        }
    }

    fn beam(pos: (i16, i16), damage: i32) -> Shot {
        Shot {
            speed: -3,
            pierce: true,
            kind: ShotKind::Beam,
            ..Shot::bolt(pos, 0, damage)
        }
    }

    fn missile(pos: (i16, i16), damage: i32) -> Shot {
        Shot {
            speed: -1,
            homing: true,
            kind: ShotKind::Missile,
            ..Shot::bolt(pos, 0, damage)
        }
    }

    fn plasma(pos: (i16, i16), damage: i32, half_width: i16) -> Shot {
        Shot {
            speed: -1,
            half_width,
            kind: ShotKind::Plasma,
            ..Shot::bolt(pos, 0, damage)
        }
    }

    fn enemy(pos: (i16, i16), drift: i16, speed: i16) -> Shot {
        Shot {
            speed,
            kind: ShotKind::Enemy,
            ..Shot::bolt(pos, drift, 1)
        }
    }

    /// A shot heavy enough to cost the hull two shield pips instead of one.
    fn heavy(mut self) -> Shot {
        self.damage = 2;
        self
    }

    /// Nebula soup costs every shot a row of speed, to a floor of one.
    fn slow(&mut self) {
        let dir = self.speed.signum();
        let rows = self.speed.abs();
        self.speed = dir * (rows - 1).max(1);
    }
}

/// The hulls that fly against you.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnemyKind {
    Grunt,
    Weaver,
    Turret,
    Bomber,
    Kamikaze,
    Tank,
    /// Telegraphs, then throws a shot twice as fast as anything else.
    Sniper,
    /// Leaves mines hanging in the court behind it.
    Miner,
    /// Breaks into two diving grunts when it dies.
    Splitter,
    /// Repairs damaged hulls flying near it.
    Healer,
}

impl EnemyKind {
    pub fn hp(self) -> i32 {
        match self {
            EnemyKind::Grunt | EnemyKind::Kamikaze => 1,
            EnemyKind::Weaver | EnemyKind::Sniper => 2,
            EnemyKind::Turret | EnemyKind::Miner | EnemyKind::Splitter | EnemyKind::Healer => 3,
            EnemyKind::Bomber => 4,
            EnemyKind::Tank => 7,
        }
    }

    pub fn score(self) -> u32 {
        match self {
            EnemyKind::Grunt => 10,
            EnemyKind::Weaver => 20,
            EnemyKind::Kamikaze => 25,
            EnemyKind::Turret => 30,
            EnemyKind::Miner => 35,
            EnemyKind::Bomber => 40,
            EnemyKind::Splitter => 40,
            EnemyKind::Sniper => 45,
            EnemyKind::Healer => 50,
            EnemyKind::Tank => 80,
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            EnemyKind::Grunt => "ᴥ",
            EnemyKind::Weaver => "ʬ",
            EnemyKind::Turret => "⊕",
            EnemyKind::Bomber => "҂",
            EnemyKind::Kamikaze => "ѵ",
            EnemyKind::Tank => "Ѫ",
            EnemyKind::Sniper => "⌖",
            EnemyKind::Miner => "Ѳ",
            EnemyKind::Splitter => "Ж",
            EnemyKind::Healer => "✚",
        }
    }

    /// 1-in-N chance per tick of peeling out of formation; `0` never dives.
    fn dive_chance(self) -> u64 {
        match self {
            EnemyKind::Grunt => 140,
            EnemyKind::Kamikaze => 70,
            EnemyKind::Bomber => 220,
            EnemyKind::Splitter => 160,
            _ => 0,
        }
    }

    /// 1-in-N chance per tick of shooting (or, for a miner, of dropping a
    /// mine); `0` never does.
    fn fire_chance(self) -> u64 {
        match self {
            EnemyKind::Grunt => 180,
            EnemyKind::Turret => 60,
            EnemyKind::Bomber => 90,
            EnemyKind::Tank => 120,
            EnemyKind::Sniper => 100,
            EnemyKind::Miner => 150,
            _ => 0,
        }
    }

    /// Rows a diving hull of this kind covers per tick.
    fn dive_speed(self) -> i16 {
        match self {
            EnemyKind::Kamikaze => 2,
            _ => 1,
        }
    }
}

/// What an enemy is doing right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnemyState {
    /// Holding station in the swaying formation.
    Formation,
    /// Peeled off and diving at the ship's column.
    Diving { target_x: i16 },
    /// Snaking down the court, bouncing off the walls.
    Weaving { dir: i16 },
}

/// One enemy hull on the court.
#[derive(Clone, Debug)]
pub struct Enemy {
    pub kind: EnemyKind,
    pub pos: (i16, i16),
    /// Its formation slot, which a diver returns to if it survives the run.
    pub home: (i16, i16),
    pub hp: i32,
    pub max_hp: i32,
    pub state: EnemyState,
    /// Ticks a sniper still has to telegraph before its shot goes off.
    pub charge: u32,
}

impl Enemy {
    pub fn new(kind: EnemyKind, home: (i16, i16)) -> Enemy {
        Enemy {
            kind,
            pos: home,
            home,
            hp: kind.hp(),
            max_hp: kind.hp(),
            state: EnemyState::Formation,
            charge: 0,
        }
    }
}

/// The shape a wave holds station in; they cycle so no two waves in a row look
/// the same.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Formation {
    Grid,
    Vee,
    Diamond,
    Columns,
    Arc,
}

impl Formation {
    pub fn of_wave(wave: u32) -> Formation {
        match wave % 5 {
            0 => Formation::Arc,
            1 => Formation::Grid,
            2 => Formation::Vee,
            3 => Formation::Diamond,
            _ => Formation::Columns,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Formation::Grid => "grid",
            Formation::Vee => "vee",
            Formation::Diamond => "diamond",
            Formation::Columns => "columns",
            Formation::Arc => "arc",
        }
    }

    /// Board cell of the formation slot at grid position `(row, col)`.
    pub fn slot(self, row: usize, col: usize) -> (i16, i16) {
        let r = row as i16;
        let c = col as i16;
        let centre = (COLS as i16 - 1) / 2;
        let spread = (c - centre).abs();
        let x = BASE_X + c * ENEMY_GAP;
        let y = match self {
            Formation::Grid => FORMATION_TOP + r * 2,
            Formation::Vee => FORMATION_TOP + r * 2 + spread,
            Formation::Diamond => FORMATION_TOP + r * 2 + (centre - spread),
            Formation::Columns => FORMATION_TOP + r * 3 + c % 2,
            Formation::Arc => FORMATION_TOP + r * 2 + spread * spread / 6,
        };
        (y, x)
    }
}

/// The stretch of space a wave is fought in. Sectors cycle, and each one
/// changes what is in the court as well as what it looks like.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sector {
    OpenSpace,
    AsteroidBelt,
    Nebula,
    Minefield,
    IonStorm,
    DebrisRing,
    /// Close to the star: flares sweep the court in pairs.
    SolarCorona,
    /// A graveyard of hulks and old mines.
    Wreckage,
    /// The tail of a comet, thick with ice and rock.
    CometTrail,
    /// A tear in space: wells drag at everything, and the stars are gone.
    VoidRift,
}

impl Sector {
    pub const ALL: [Sector; 10] = [
        Sector::OpenSpace,
        Sector::AsteroidBelt,
        Sector::Nebula,
        Sector::Minefield,
        Sector::IonStorm,
        Sector::DebrisRing,
        Sector::SolarCorona,
        Sector::Wreckage,
        Sector::CometTrail,
        Sector::VoidRift,
    ];

    pub fn of_wave(wave: u32) -> Sector {
        Sector::ALL[(wave as usize + 9) % Sector::ALL.len()]
    }

    pub fn name(self) -> &'static str {
        match self {
            Sector::OpenSpace => "open space",
            Sector::AsteroidBelt => "asteroid belt",
            Sector::Nebula => "nebula",
            Sector::Minefield => "minefield",
            Sector::IonStorm => "ion storm",
            Sector::DebrisRing => "debris ring",
            Sector::SolarCorona => "solar corona",
            Sector::Wreckage => "wreckage",
            Sector::CometTrail => "comet trail",
            Sector::VoidRift => "void rift",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Sector::OpenSpace => "clear lanes, nothing but the formation",
            Sector::AsteroidBelt => "rocks pouring down the court",
            Sector::Nebula => "soup: every shot flies a row slower",
            Sector::Minefield => "mines already hanging in the lanes",
            Sector::IonStorm => "surges drain the reactor and stun the drones",
            Sector::DebrisRing => "hulks that eat shots and block the lanes",
            Sector::SolarCorona => "two walls of fire sweeping the court",
            Sector::Wreckage => "a graveyard of hulks and old mines",
            Sector::CometTrail => "ice and rock, pouring past",
            Sector::VoidRift => "wells that drag at the hull, and no stars",
        }
    }

    /// 1-in-N odds per tick that a rock drifts in.
    fn asteroid_chance(self) -> u64 {
        match self {
            Sector::CometTrail => 24,
            Sector::AsteroidBelt => 40,
            Sector::Wreckage => 90,
            Sector::DebrisRing => 160,
            _ => ASTEROID_CHANCE,
        }
    }

    /// Mines already scattered in the court when the wave starts.
    fn starting_mines(self) -> usize {
        match self {
            Sector::Minefield => 14,
            Sector::Wreckage => 6,
            Sector::DebrisRing => 3,
            _ => 0,
        }
    }

    /// Debris blocks drifting through the court.
    fn debris_blocks(self) -> usize {
        match self {
            Sector::Wreckage => 11,
            Sector::DebrisRing => 8,
            Sector::AsteroidBelt => 2,
            _ => 0,
        }
    }

    /// Whether the soup costs every shot a row of speed.
    fn drag(self) -> bool {
        self == Sector::Nebula
    }

    /// Ticks between ion surges; `0` never surges.
    fn surge_cadence(self) -> u32 {
        match self {
            Sector::IonStorm => 220,
            _ => 0,
        }
    }

    /// How thick the backdrop is drawn, in cells per layer.
    fn backdrop(self) -> usize {
        match self {
            Sector::Nebula => 30,
            Sector::SolarCorona => 26,
            Sector::IonStorm => 22,
            Sector::OpenSpace | Sector::CometTrail => 16,
            Sector::VoidRift => 4,
            _ => 12,
        }
    }

    /// The glyph the backdrop is drawn with at each parallax layer.
    fn star_glyph(self, layer: usize) -> &'static str {
        match (self, layer) {
            (Sector::Nebula, 0) => "░",
            (Sector::Nebula, 1) => "▒",
            (Sector::Nebula, _) => "▓",
            (Sector::SolarCorona, 0) => "░",
            (Sector::SolarCorona, 1) => "▒",
            (Sector::SolarCorona, _) => "▓",
            (Sector::CometTrail, 0) => "·",
            (Sector::CometTrail, 1) => "˙",
            (Sector::CometTrail, _) => "❄",
            (Sector::VoidRift, _) => "‧",
            (Sector::IonStorm, 0) => "·",
            (Sector::IonStorm, 1) => "¦",
            (Sector::IonStorm, _) => "⌇",
            (_, 0) => "·",
            (_, 1) => "˙",
            (_, _) => "*",
        }
    }
}

/// One backdrop cell, scrolling at its layer's speed.
#[derive(Clone, Copy, Debug)]
pub struct Star {
    pub pos: (i16, i16),
    pub layer: usize,
}

/// What a dropped pickup gives you.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerKind {
    /// A gun: the one you carry levels up, any other replaces it.
    Gun(Weapon),
    Shield,
    Bomb,
    Rapid,
    /// A wing drone that fires alongside the ship.
    Drone,
    /// Points and salvage.
    Medal,
    Life,
}

impl PowerKind {
    pub fn glyph(self) -> &'static str {
        match self {
            PowerKind::Gun(w) => w.tag(),
            PowerKind::Shield => "◈",
            PowerKind::Bomb => "◆",
            PowerKind::Rapid => "»",
            PowerKind::Drone => "◇",
            PowerKind::Medal => "★",
            PowerKind::Life => "♥",
        }
    }
}

/// A pickup tumbling down the court.
#[derive(Clone, Debug)]
pub struct Powerup {
    pub pos: (i16, i16),
    pub kind: PowerKind,
}

/// A mine hanging in the court.
#[derive(Clone, Debug)]
pub struct Mine {
    pub pos: (i16, i16),
    pub fuse: u32,
}

/// A rock drifting down the court; shoot it or fly around it.
#[derive(Clone, Debug)]
pub struct Asteroid {
    pub pos: (i16, i16),
    pub hp: i32,
    pub drift: i16,
}

/// A hulk that blocks the lane: it eats shots from both sides until it breaks.
#[derive(Clone, Debug)]
pub struct Debris {
    pub pos: (i16, i16),
    pub hp: i32,
}

/// Which of the four bosses a wave is fighting.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BossKind {
    /// One wide hull; the plain escalating sweeper.
    Dreadnought,
    /// A core armoured until both of its turrets are gone.
    Twin,
    /// Two launch bays feeding kamikazes into the court.
    Carrier,
    /// A head trailing segments that swim behind it.
    Serpent,
}

impl BossKind {
    /// The bosses cycle in this order, one per boss wave.
    pub fn of_wave(wave: u32) -> BossKind {
        match (wave / BOSS_EVERY) % 4 {
            1 => BossKind::Dreadnought,
            2 => BossKind::Twin,
            3 => BossKind::Carrier,
            _ => BossKind::Serpent,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            BossKind::Dreadnought => "dreadnought",
            BossKind::Twin => "twin",
            BossKind::Carrier => "carrier",
            BossKind::Serpent => "serpent",
        }
    }

    /// Half-width of the core hull.
    pub fn core_half(self) -> i16 {
        match self {
            BossKind::Twin => 3,
            BossKind::Serpent => 1,
            _ => 6,
        }
    }

    /// Extra rows the core hull covers below its anchor row.
    pub fn core_depth(self) -> i16 {
        match self {
            BossKind::Serpent => 0,
            _ => 1,
        }
    }
}

/// A destructible piece of a boss: a turret, a launch bay or a body segment.
#[derive(Clone, Debug)]
pub struct BossPart {
    /// Offset from the boss anchor; the serpent recomputes its own as it swims.
    pub offset: (i16, i16),
    pub hp: i32,
    pub max_hp: i32,
}

impl BossPart {
    fn new(offset: (i16, i16), hp: i32) -> BossPart {
        BossPart {
            offset,
            hp,
            max_hp: hp,
        }
    }
}

/// The wave boss: a wide hull that sweeps the top of the court and escalates
/// through three attack phases as its armour burns off.
#[derive(Clone, Debug)]
pub struct Boss {
    pub kind: BossKind,
    pub pos: (i16, i16),
    pub hp: i32,
    pub max_hp: i32,
    pub dir: i16,
    pub parts: Vec<BossPart>,
    cooldown: u32,
    minion_timer: u32,
    tick: u32,
}

impl Boss {
    pub fn new(kind: BossKind, hp: i32) -> Boss {
        let parts = match kind {
            BossKind::Dreadnought => Vec::new(),
            BossKind::Twin => vec![
                BossPart::new((0, -6), hp / 3),
                BossPart::new((0, 6), hp / 3),
            ],
            BossKind::Carrier => vec![
                BossPart::new((1, -5), hp / 4),
                BossPart::new((1, 5), hp / 4),
            ],
            BossKind::Serpent => (0..SERPENT_SEGMENTS)
                .map(|i| BossPart::new((0, -(i as i16 + 1) * 2), hp / 6))
                .collect(),
        };
        Boss {
            kind,
            pos: (FORMATION_TOP, W / 2),
            hp,
            max_hp: hp,
            dir: 1,
            parts,
            cooldown: 20,
            minion_timer: 90,
            tick: 0,
        }
    }

    /// `1` above two thirds health, `2` down to a third, `3` once enraged.
    pub fn phase(&self) -> u8 {
        match self.hp.max(0) * 3 / self.max_hp.max(1) {
            f if f >= 2 => 1,
            1 => 2,
            _ => 3,
        }
    }

    /// The twin's core cannot be touched while either turret still stands.
    pub fn armoured(&self) -> bool {
        self.kind == BossKind::Twin && !self.parts.is_empty()
    }

    /// Where a part sits relative to the anchor. The serpent's body swims, so
    /// its offsets come from the tick rather than the part.
    pub fn part_offset(&self, index: usize, part: &BossPart) -> (i16, i16) {
        match self.kind {
            BossKind::Serpent => {
                let phase = (self.tick as usize / 2 + index * 2) % SERPENT_WAVE.len();
                (SERPENT_WAVE[phase], -self.dir * (index as i16 + 1) * 2)
            }
            _ => part.offset,
        }
    }

    /// Every live part's cell on the board, in part order.
    pub fn part_cells(&self) -> Vec<(i16, i16)> {
        self.parts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let off = self.part_offset(i, p);
                (self.pos.0 + off.0, self.pos.1 + off.1)
            })
            .collect()
    }

    /// Columns it sweeps per tick.
    fn speed(&self) -> i16 {
        match (self.kind, self.phase()) {
            (BossKind::Serpent, 3) => 3,
            (BossKind::Serpent, _) => 2,
            (BossKind::Carrier, _) => 1,
            (_, 3) => 2,
            _ => 1,
        }
    }

    /// Ticks between its volleys.
    fn cadence(&self) -> u32 {
        match self.phase() {
            1 => 14,
            2 => 10,
            _ => 6,
        }
    }
}

/// Where a round is: building a ship, flying, counting a cleared wave down,
/// spending salvage in the hangar, or over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Select,
    Playing,
    WaveClear,
    Hangar,
    Lost,
}

/// The permanent upgrade a pilot level hands out; they rotate in this order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LevelReward {
    /// One more shield pip, filled in on the spot.
    Plating,
    /// One more point of damage on every shot.
    Firepower,
    /// Faster energy regeneration, so the special comes round sooner.
    Cell,
    /// A spare smart bomb.
    Bomb,
}

impl LevelReward {
    /// What the pilot gets for reaching `level`.
    pub fn of_level(level: u32) -> LevelReward {
        match level % 4 {
            1 => LevelReward::Plating,
            2 => LevelReward::Firepower,
            3 => LevelReward::Cell,
            _ => LevelReward::Bomb,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            LevelReward::Plating => "hull plating",
            LevelReward::Firepower => "firepower",
            LevelReward::Cell => "energy cell",
            LevelReward::Bomb => "smart bomb",
        }
    }
}

/// A consumable the hangar sells alongside the ship components.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stock {
    Repair,
    GunLevel,
    GunSwap,
    Drone,
    Bomb,
    Rapid,
    Life,
}

impl Stock {
    pub const ALL: [Stock; 7] = [
        Stock::Repair,
        Stock::GunLevel,
        Stock::GunSwap,
        Stock::Drone,
        Stock::Bomb,
        Stock::Rapid,
        Stock::Life,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Stock::Repair => "shield repair",
            Stock::GunLevel => "gun level",
            Stock::GunSwap => "swap gun",
            Stock::Drone => "wing drone",
            Stock::Bomb => "smart bomb",
            Stock::Rapid => "rapid fire",
            Stock::Life => "spare hull",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Stock::Repair => "refill every shield pip",
            Stock::GunLevel => "+1 level on the gun you carry",
            Stock::GunSwap => "rotate to the next gun",
            Stock::Drone => "a wing drone that fires with you",
            Stock::Bomb => "+1 smart bomb",
            Stock::Rapid => "rapid fire through the next wave",
            Stock::Life => "+1 life",
        }
    }

    pub fn price(self) -> u32 {
        match self {
            Stock::Repair => 150,
            Stock::GunSwap => 200,
            Stock::Bomb => 250,
            Stock::Rapid => 300,
            Stock::GunLevel => 500,
            Stock::Drone => 600,
            Stock::Life => 1200,
        }
    }
}

/// One line in the hangar: a component tier, a module, or a consumable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShopEntry {
    Component(Part),
    Fitting(Module),
    Consumable(Stock),
}

/// A hangar line as the renderer needs it.
#[derive(Clone, Debug)]
pub struct ShopLine {
    pub key: char,
    pub entry: ShopEntry,
    pub label: String,
    pub detail: &'static str,
    pub price: u32,
    /// False when it is maxed out, already fitted, or unaffordable.
    pub available: bool,
}

/// How the rock is laid out in a sector: the shape of the flyable channel.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TerrainKind {
    /// Nothing but the court walls.
    Open,
    /// A channel that pinches shut and opens back up.
    Canyon,
    /// A wide, wandering passage with a rough edge.
    Cave,
    /// A narrow run with almost no room either side.
    Tunnel,
    /// A wide channel studded with rock columns.
    Pillars,
    /// Bulkheads across the court with a single gap to thread.
    Gates,
    /// A rock spine down the middle, splitting the court in two.
    Spine,
    /// Alternating half-walls that force the hull to weave.
    Maze,
    /// Thick with columns, wall to wall.
    Reef,
}

impl TerrainKind {
    pub const ALL: [TerrainKind; 9] = [
        TerrainKind::Open,
        TerrainKind::Canyon,
        TerrainKind::Cave,
        TerrainKind::Tunnel,
        TerrainKind::Pillars,
        TerrainKind::Gates,
        TerrainKind::Spine,
        TerrainKind::Maze,
        TerrainKind::Reef,
    ];

    pub fn name(self) -> &'static str {
        match self {
            TerrainKind::Open => "open",
            TerrainKind::Canyon => "canyon",
            TerrainKind::Cave => "cave",
            TerrainKind::Tunnel => "tunnel",
            TerrainKind::Pillars => "pillars",
            TerrainKind::Gates => "gates",
            TerrainKind::Spine => "spine",
            TerrainKind::Maze => "maze",
            TerrainKind::Reef => "reef",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            TerrainKind::Open => "clear lanes wall to wall",
            TerrainKind::Canyon => "a channel that pinches shut",
            TerrainKind::Cave => "a wandering passage with rough edges",
            TerrainKind::Tunnel => "barely wider than the hull",
            TerrainKind::Pillars => "open, but studded with rock columns",
            TerrainKind::Gates => "bulkheads with one gap to thread",
            TerrainKind::Spine => "a rock spine splitting the court",
            TerrainKind::Maze => "half-walls that force a weave",
            TerrainKind::Reef => "columns wall to wall",
        }
    }

    /// Narrowest and widest the channel gets, in columns.
    fn width_range(self) -> (i16, i16) {
        match self {
            TerrainKind::Open => (W - 2, W - 2),
            TerrainKind::Canyon => (26, W - 8),
            TerrainKind::Cave => (28, W - 4),
            TerrainKind::Tunnel => (22, 32),
            TerrainKind::Pillars | TerrainKind::Reef => (W - 8, W - 4),
            TerrainKind::Gates | TerrainKind::Spine | TerrainKind::Maze => (W - 6, W - 2),
        }
    }

    /// Columns the channel centre may wander per row.
    fn wander(self) -> i16 {
        match self {
            TerrainKind::Open => 0,
            TerrainKind::Canyon | TerrainKind::Tunnel => 1,
            TerrainKind::Cave => 2,
            TerrainKind::Pillars | TerrainKind::Reef => 1,
            TerrainKind::Gates | TerrainKind::Spine => 2,
            TerrainKind::Maze => 0,
        }
    }

    /// Whether shooting the rock carves it away.
    fn destructible(self) -> bool {
        matches!(
            self,
            TerrainKind::Cave | TerrainKind::Pillars | TerrainKind::Reef | TerrainKind::Spine
        )
    }

    /// 1-in-N odds a generated row grows a rock column in the channel.
    fn pillar_chance(self) -> u64 {
        match self {
            TerrainKind::Reef => 2,
            TerrainKind::Pillars => 4,
            TerrainKind::Cave => 14,
            _ => 0,
        }
    }
}

/// One row of the scrolling rock: the open channel, plus any column standing in
/// the middle of it.
#[derive(Clone, Debug)]
pub struct TerrainRow {
    /// Leftmost and rightmost flyable columns, inclusive.
    pub open: (i16, i16),
    /// Rock columns standing inside the channel, if this row grew any.
    pub pillars: Vec<i16>,
}

impl TerrainRow {
    fn solid(&self, col: i16) -> bool {
        col < self.open.0 || col > self.open.1 || self.pillars.contains(&col)
    }
}

/// The rock the sector is flown through: one row per board row, scrolling down
/// the court as the ship flies up the map.
#[derive(Clone, Debug)]
pub struct Terrain {
    pub kind: TerrainKind,
    /// Indexed by board row; row zero is the top of the court.
    pub rows: Vec<TerrainRow>,
    centre: i16,
    width: i16,
    drift: i16,
    squeeze: i16,
    /// Rows generated so far, which is what paces gates and maze walls.
    phase: u32,
    rng: u64,
}

impl Terrain {
    pub fn new(kind: TerrainKind, seed: u64) -> Terrain {
        let (min, max) = kind.width_range();
        let mut terrain = Terrain {
            kind,
            rows: Vec::with_capacity(H as usize),
            centre: W / 2,
            width: max,
            drift: 1,
            squeeze: -1,
            phase: 0,
            rng: seed | 1,
            //
        };
        let _ = min;
        for _ in 0..H {
            let row = terrain.generate();
            terrain.rows.push(row);
        }
        terrain
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Walk the channel one row on: the centre wanders, the width breathes
    /// between the kind's limits, and some rows grow a pillar.
    fn generate(&mut self) -> TerrainRow {
        self.phase = self.phase.wrapping_add(1);
        match self.kind {
            // A bulkhead every few rows, with one gap to thread.
            TerrainKind::Gates => {
                return if self.phase.is_multiple_of(GATE_PERIOD) {
                    let gap = GATE_GAP;
                    let centre = (self.rand() % (W as u64 - gap as u64 - 4)) as i16 + gap / 2 + 2;
                    TerrainRow {
                        open: (centre - gap / 2, centre + gap / 2),
                        pillars: Vec::new(),
                    }
                } else {
                    TerrainRow {
                        open: (1, W - 2),
                        pillars: Vec::new(),
                    }
                };
            }
            // A spine of rock down the middle, wandering as it goes.
            TerrainKind::Spine => {
                if self.rand().is_multiple_of(4) {
                    self.drift = -self.drift;
                }
                self.centre = (self.centre + self.drift).clamp(8, W - 9);
                return TerrainRow {
                    open: (2, W - 3),
                    pillars: vec![self.centre, self.centre + 1],
                };
            }
            // Half-walls, alternating sides every few rows.
            TerrainKind::Maze => {
                let block = self.phase / MAZE_PERIOD;
                let open = if self.phase % MAZE_PERIOD >= MAZE_WALL {
                    (1, W - 2)
                } else if block.is_multiple_of(2) {
                    (W / 4, W - 2)
                } else {
                    (1, 3 * W / 4)
                };
                return TerrainRow {
                    open,
                    pillars: Vec::new(),
                };
            }
            _ => {}
        }
        let (min, max) = self.kind.width_range();
        let wander = self.kind.wander();
        if wander > 0 {
            if self.rand().is_multiple_of(3) {
                self.drift = -self.drift;
            }
            self.centre = (self.centre + self.drift * wander).clamp(min / 2 + 2, W - min / 2 - 2);
            self.width = (self.width + self.squeeze).clamp(min, max);
            if self.width == min || self.width == max {
                self.squeeze = -self.squeeze;
            }
        }
        let half = self.width / 2;
        let open = ((self.centre - half).max(1), (self.centre + half).min(W - 2));
        let chance = self.kind.pillar_chance();
        let mut pillars = Vec::new();
        if chance > 0 {
            let span = (open.1 - open.0).max(2) as u64;
            for _ in 0..PILLARS_PER_ROW {
                if self.rand().is_multiple_of(chance) {
                    let col = open.0 + 1 + (self.rand() % (span - 1)) as i16;
                    if !pillars.contains(&col) {
                        pillars.push(col);
                    }
                }
            }
        }
        TerrainRow { open, pillars }
    }

    /// Scroll the rock one row down the court, generating a fresh row on top.
    pub fn scroll(&mut self) {
        let row = self.generate();
        self.rows.pop();
        self.rows.insert(0, row);
    }

    /// Is the cell rock?
    pub fn solid(&self, r: i16, c: i16) -> bool {
        if self.kind == TerrainKind::Open {
            return false;
        }
        match self.rows.get(r.max(0) as usize) {
            Some(row) => row.solid(c),
            None => false,
        }
    }

    /// Blast a cell of rock away, where the kind allows it.
    pub fn carve(&mut self, r: i16, c: i16) -> bool {
        if !self.kind.destructible() {
            return false;
        }
        let Some(row) = self.rows.get_mut(r.max(0) as usize) else {
            return false;
        };
        if let Some(i) = row.pillars.iter().position(|&p| p == c) {
            row.pillars.remove(i);
            return true;
        }
        if c < row.open.0 {
            row.open.0 = (row.open.0 - 1).max(1);
            return true;
        }
        if c > row.open.1 {
            row.open.1 = (row.open.1 + 1).min(W - 2);
            return true;
        }
        false
    }

    /// The flyable span on a row, for keeping ships and spawns inside it.
    pub fn channel(&self, r: i16) -> (i16, i16) {
        match self.rows.get(r.max(0) as usize) {
            Some(row) if self.kind != TerrainKind::Open => row.open,
            _ => (1, W - 2),
        }
    }
}

/// A gun emplacement bolted to the rock, riding down with the scroll.
#[derive(Clone, Debug)]
pub struct WallTurret {
    pub pos: (i16, i16),
    pub hp: i32,
    cooldown: u32,
}

/// Something the sector itself does to you, over and above the rock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hazard {
    /// Drags the hull a column toward it every few ticks.
    GravityWell { pos: (i16, i16) },
    /// A wall of fire sweeping across the court a column at a time.
    SolarFlare { col: i16, dir: i16 },
    /// A current that shoves the hull sideways.
    IonStream { push: i16 },
}

impl Hazard {
    pub fn name(self) -> &'static str {
        match self {
            Hazard::GravityWell { .. } => "gravity well",
            Hazard::SolarFlare { .. } => "solar flare",
            Hazard::IonStream { .. } => "ion stream",
        }
    }
}

/// What a route node pays out when it is flown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeBonus {
    /// A salvage cache banked the moment the wave starts.
    Cache(u32),
    /// A gun crate: the node hands over a weapon.
    Armoury(Weapon),
    /// Shields and a bomb, topped up on arrival.
    Refit,
    /// Thicker enemy armour, double salvage from every kill.
    Danger,
}

impl NodeBonus {
    pub fn label(self) -> String {
        match self {
            NodeBonus::Cache(credits) => format!("salvage cache ({credits})"),
            NodeBonus::Armoury(w) => format!("armoury ({})", w.name()),
            NodeBonus::Refit => "refit (shields + bomb)".to_string(),
            NodeBonus::Danger => "danger (armoured, double salvage)".to_string(),
        }
    }
}

/// One stop on the star chart: where you fly next, and what is waiting there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RouteNode {
    pub sector: Sector,
    pub terrain: TerrainKind,
    pub bonus: NodeBonus,
}

impl RouteNode {
    pub fn label(&self) -> String {
        format!(
            "{} · {} — {}",
            self.sector.name(),
            self.terrain.name(),
            self.bonus.label()
        )
    }
}

/// The pure Nova court. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    pub class: ShipClass,
    pub difficulty: Difficulty,
    /// Everything bolted to the ship, bought in the hangar.
    pub loadout: Loadout,
    /// Ship position as `(row, col)`; it flies the bottom seven rows.
    pub ship: (i16, i16),
    pub weapon: Weapon,
    pub weapon_level: u32,
    pub shield: u32,
    pub max_shield: u32,
    pub lives: u32,
    pub bombs: u32,
    /// Wing drones riding either side of the hull, as `-1`/`1` sides.
    pub drones: Vec<i16>,
    pub score: u32,
    /// Salvage banked for the hangar.
    pub credits: u32,
    pub xp: u32,
    pub level: u32,
    /// Experience still owed for the next level.
    pub xp_next: u32,
    /// Kill-chain multiplier, `1` when cold.
    pub combo: u32,
    pub medals: u32,
    pub energy: u32,
    pub wave: u32,
    pub sector: Sector,
    pub formation: Formation,
    pub status: Status,
    pub enemies: Vec<Enemy>,
    pub boss: Option<Boss>,
    pub shots: Vec<Shot>,
    pub enemy_shots: Vec<Shot>,
    pub powerups: Vec<Powerup>,
    pub mines: Vec<Mine>,
    pub asteroids: Vec<Asteroid>,
    pub debris: Vec<Debris>,
    pub stars: Vec<Star>,
    /// The rock this leg of the route is flown through.
    pub terrain: Terrain,
    /// Guns bolted to that rock.
    pub turrets: Vec<WallTurret>,
    /// Whatever else the sector is doing to the hull.
    pub hazards: Vec<Hazard>,
    /// The stop being flown, and the three the chart offers next.
    pub node: RouteNode,
    pub route: Vec<RouteNode>,
    /// Ticks the arrival banner still has to run.
    pub banner: u32,
    /// Frames of flash left over from a bomb or a surge, for the renderer.
    pub flash: u32,
    /// Ticks left of the cleared-wave pause.
    pub intermission: u32,
    /// Ticks the bulwark still holds.
    pub bulwark: u32,
    /// Ticks the drones are still knocked out by an ion surge.
    pub drone_stun: u32,
    /// Extra shield pips and damage handed out by pilot levels.
    bonus_plating: u32,
    bonus_damage: i32,
    bonus_regen: u32,
    sway_x: i16,
    sway_dir: i16,
    sway_counter: u32,
    fire_cooldown: u32,
    invuln: u32,
    combo_timer: u32,
    rapid: u32,
    repair_timer: u32,
    facing: i16,
    next_extend: u32,
    /// Ticks flown this run; the renderer reads it for the hazard rhythms.
    pub tick: u32,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        Game {
            class: ShipClass::Cruiser,
            difficulty: Difficulty::Normal,
            loadout: Loadout::default(),
            ship: (SHIP_ROW, W / 2),
            weapon: Weapon::Blaster,
            weapon_level: 1,
            shield: 0,
            max_shield: 0,
            lives: 3,
            bombs: 0,
            drones: Vec::new(),
            score: 0,
            credits: 0,
            xp: 0,
            level: 1,
            xp_next: XP_PER_LEVEL,
            combo: 1,
            medals: 0,
            energy: BASE_ENERGY,
            wave: 1,
            sector: Sector::of_wave(1),
            formation: Formation::of_wave(1),
            status: Status::Select,
            enemies: Vec::new(),
            boss: None,
            shots: Vec::new(),
            enemy_shots: Vec::new(),
            powerups: Vec::new(),
            mines: Vec::new(),
            asteroids: Vec::new(),
            debris: Vec::new(),
            stars: Vec::new(),
            terrain: Terrain::new(TerrainKind::Open, seed),
            turrets: Vec::new(),
            hazards: Vec::new(),
            node: RouteNode {
                sector: Sector::of_wave(1),
                terrain: TerrainKind::Open,
                bonus: NodeBonus::Refit,
            },
            route: Vec::new(),
            banner: 0,
            flash: 0,
            intermission: 0,
            bulwark: 0,
            drone_stun: 0,
            bonus_plating: 0,
            bonus_damage: 0,
            bonus_regen: 0,
            sway_x: 0,
            sway_dir: 1,
            sway_counter: SWAY_CADENCE,
            fire_cooldown: 0,
            invuln: 0,
            combo_timer: 0,
            rapid: 0,
            repair_timer: REPAIR_CADENCE,
            facing: 1,
            next_extend: EXTEND_SCORE,
            tick: 0,
            rng: seed | 1,
        }
    }

    /// Commit to a hull and a difficulty, and fly wave one.
    pub fn start(&mut self, class: ShipClass, difficulty: Difficulty) {
        self.class = class;
        self.difficulty = difficulty;
        self.loadout = Loadout::default();
        self.ship = (SHIP_ROW, W / 2);
        self.weapon = Weapon::Blaster;
        self.weapon_level = 1;
        self.bonus_plating = 0;
        self.bonus_damage = 0;
        self.bonus_regen = 0;
        self.recompute_shield();
        self.shield = self.max_shield;
        self.bombs = class.bombs();
        self.drones.clear();
        self.lives = 3;
        self.score = 0;
        self.credits = 0;
        self.xp = 0;
        self.level = 1;
        self.xp_next = XP_PER_LEVEL;
        self.combo = 1;
        self.medals = 0;
        self.energy = self.max_energy();
        self.next_extend = EXTEND_SCORE;
        self.wave = 1;
        self.route.clear();
        self.node = RouteNode {
            sector: Sector::of_wave(1),
            terrain: TerrainKind::Open,
            bonus: NodeBonus::Refit,
        };
        self.status = Status::Playing;
        self.spawn_wave();
    }

    /// The same LCG PRNG the snake port uses.
    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Difficulty- and wave-scaled 1-in-N odds: a harder run, and a later
    /// wave, both mean a busier court.
    fn odds(&self, base: u64) -> u64 {
        let pressure = self.difficulty.aggression() + (self.wave / 5) as u64;
        (base / pressure).max(6)
    }

    /// Armour every enemy hull carries: the difficulty sets the floor and the
    /// campaign thickens it every few waves, so a maxed-out build still has to
    /// work for its kills.
    fn wave_armour(&self) -> i32 {
        let danger = if self.danger() { 2 } else { 0 };
        self.difficulty.armour() + (self.wave / 3) as i32 + danger
    }

    /// Columns the hull covers per keypress, engine included.
    pub fn thrust(&self) -> i16 {
        self.class.speed() + (self.loadout.tier(Part::Engine) / 2) as i16
    }

    /// Ticks between shots, magazine and autoloader included.
    pub fn cadence(&self) -> u32 {
        let mut ticks = self
            .class
            .fire_cadence()
            .saturating_sub(self.loadout.tier(Part::Magazine) / 2);
        if self.loadout.has(Module::Autoloader) {
            ticks = ticks.saturating_sub(1);
        }
        (ticks as i32 + self.weapon.cadence_shift()).max(1) as u32
    }

    /// Damage the gun deals before its level bonus, cannon included.
    pub fn gun_damage(&self) -> i32 {
        self.class.damage() + self.loadout.tier(Part::Cannon) as i32 + self.bonus_damage
    }

    /// Energy recovered per tick, reactor included.
    pub fn regen(&self) -> u32 {
        ENERGY_REGEN + self.loadout.tier(Part::Reactor) + self.bonus_regen
    }

    /// Energy the meter holds, reactor included.
    pub fn max_energy(&self) -> u32 {
        BASE_ENERGY + 10 * self.loadout.tier(Part::Reactor)
    }

    /// What the hull special costs, overdrive included.
    pub fn special_cost(&self) -> u32 {
        if self.loadout.has(Module::Overdrive) {
            SPECIAL_COST * 2 / 3
        } else {
            SPECIAL_COST
        }
    }

    /// Recompute the shield ceiling from the hull, its plating and the levels.
    fn recompute_shield(&mut self) {
        self.max_shield =
            (self.class.max_shield() + self.loadout.tier(Part::Plating) + self.bonus_plating)
                .min(MAX_SHIELD_PIPS);
        self.shield = self.shield.min(self.max_shield);
    }

    /// A fresh enemy of `kind`, carrying the difficulty's extra armour.
    fn hatch(&self, kind: EnemyKind, home: (i16, i16)) -> Enemy {
        let mut e = Enemy::new(kind, home);
        e.max_hp += self.wave_armour();
        e.hp = e.max_hp;
        e
    }

    /// True while the ship is still flashing from its last hit.
    pub fn invulnerable(&self) -> bool {
        self.invuln > 0
    }

    /// True while a rapid-fire pickup is running.
    pub fn rapid_active(&self) -> bool {
        self.rapid > 0
    }

    /// True while the hull special is paid for and ready.
    pub fn special_ready(&self) -> bool {
        self.energy >= self.special_cost()
    }

    /// Which hull sits in a formation slot: later waves seed tougher ones, and
    /// the front row is the one that peels off and dives.
    fn wave_kind(&self, row: usize, col: usize) -> EnemyKind {
        let w = self.wave;
        match row {
            0 if w >= 2 && col % 3 == 1 => EnemyKind::Turret,
            0 if w >= 6 && col % 4 == 3 => EnemyKind::Sniper,
            0 => EnemyKind::Grunt,
            1 if w >= 3 && col % 4 == 2 => EnemyKind::Bomber,
            1 if w >= 7 && col % 5 == 4 => EnemyKind::Miner,
            1 => EnemyKind::Weaver,
            2 if w >= 5 && col % 5 == 0 => EnemyKind::Tank,
            2 if w >= 9 && col % 4 == 2 => EnemyKind::Healer,
            2 if w >= 6 && col % 3 == 1 => EnemyKind::Splitter,
            2 => EnemyKind::Grunt,
            _ => EnemyKind::Kamikaze,
        }
    }

    /// Lay out the sector: its backdrop, and whatever hazards it starts with.
    fn dress_sector(&mut self) {
        self.stars.clear();
        for layer in 0..STAR_LAYERS {
            for _ in 0..self.sector.backdrop() {
                let r = (self.rand() % H as u64) as i16;
                let c = (self.rand() % W as u64) as i16;
                self.stars.push(Star { pos: (r, c), layer });
            }
        }
        for _ in 0..self.sector.starting_mines() {
            let r = (self.rand() % (SHIP_TOP as u64 - 2)) as i16 + 2;
            let c = (self.rand() % (W as u64 - 4)) as i16 + 2;
            self.mines.push(Mine {
                pos: (r, c),
                fuse: MINE_FUSE * 3,
            });
        }
        for _ in 0..self.sector.debris_blocks() {
            let r = (self.rand() % (SHIP_TOP as u64 - 2)) as i16 + 1;
            let c = (self.rand() % (W as u64 - 4)) as i16 + 2;
            self.debris.push(Debris {
                pos: (r, c),
                hp: DEBRIS_HP + self.wave_armour(),
            });
        }
    }

    /// Build the current wave: a boss with a kamikaze escort every
    /// `BOSS_EVERY`th wave, otherwise a mixed formation in the wave's shape.
    pub fn spawn_wave(&mut self) {
        self.enemies.clear();
        self.boss = None;
        self.shots.clear();
        self.enemy_shots.clear();
        self.powerups.clear();
        self.mines.clear();
        self.asteroids.clear();
        self.debris.clear();
        self.sway_x = 0;
        self.sway_dir = 1;
        self.sway_counter = SWAY_CADENCE;
        self.sector = self.node.sector;
        self.formation = Formation::of_wave(self.wave);
        self.banner = BANNER_TICKS;
        self.dress_sector();
        self.dress_terrain();
        self.claim_node_bonus();
        if self.wave.is_multiple_of(BOSS_EVERY) {
            let kind = BossKind::of_wave(self.wave);
            let hp = 60 + 40 * (self.wave / BOSS_EVERY) as i32 + 20 * self.wave_armour();
            self.boss = Some(Boss::new(kind, hp));
            for col in (0..COLS).step_by(2) {
                let home = self.place((FORMATION_TOP + 5, BASE_X + col as i16 * ENEMY_GAP));
                let escort = self.hatch(EnemyKind::Kamikaze, home);
                self.enemies.push(escort);
            }
            return;
        }
        // Later waves come deeper as well as tougher, but a tight map fields a
        // narrower wave: there is only so much room between the rock.
        let rows = (2 + (self.wave / 3) as usize).min(ROWS);
        let span: i16 = (0..rows as i16)
            .map(|r| {
                let (l, right) = self.terrain.channel(FORMATION_TOP + r * 2);
                right - l
            })
            .min()
            .unwrap_or(W - 2);
        let lanes = ((COLS as i16 * span / (W - 2)).clamp(4, COLS as i16)) as usize;
        for row in 0..rows {
            for col in 0..lanes {
                let kind = self.wave_kind(row, col);
                let home = self.place(self.formation.slot(row, col));
                let hull = self.hatch(kind, home);
                self.enemies.push(hull);
            }
        }
    }

    /// Leave the hangar and fly the next wave, keeping everything bought.
    pub fn launch_next_wave(&mut self) {
        self.wave += 1;
        self.shield = self.max_shield;
        self.bombs += 1;
        self.energy = self.max_energy();
        self.drone_stun = 0;
        self.status = Status::Playing;
        self.spawn_wave();
    }

    /// Bank points, paying out a spare life on every extend threshold crossed.
    fn add_score(&mut self, points: u32) {
        self.score += points * self.difficulty.score_bonus();
        while self.score >= self.next_extend {
            self.lives += 1;
            self.next_extend += EXTEND_SCORE;
        }
    }

    /// Bank experience and salvage, levelling the pilot up when the bar fills.
    fn gain_xp(&mut self, base: u32) {
        let mut salvage = base / 2 + 5;
        if self.loadout.has(Module::Salvager) {
            salvage += salvage / 2;
        }
        if self.danger() {
            salvage *= 2;
        }
        self.credits += salvage;
        self.xp += base / 4 + 1;
        while self.xp >= self.xp_next {
            self.xp -= self.xp_next;
            self.level += 1;
            self.xp_next = XP_PER_LEVEL * self.level;
            match LevelReward::of_level(self.level) {
                LevelReward::Plating => {
                    self.bonus_plating += 1;
                    self.recompute_shield();
                    self.shield += 1;
                }
                LevelReward::Firepower => self.bonus_damage += 1,
                LevelReward::Cell => self.bonus_regen += 1,
                LevelReward::Bomb => self.bombs += 1,
            }
        }
    }

    /// Fly the ship, clamped to the court and to its own bottom-third box.
    pub fn move_ship(&mut self, dc: i16, dr: i16) {
        if self.status != Status::Playing {
            return;
        }
        if dc != 0 {
            self.facing = dc.signum();
        }
        let wanted = (self.ship.1 + dc * self.thrust()).clamp(1, W - 2);
        let row = (self.ship.0 + dr).clamp(SHIP_TOP, SHIP_ROW);
        let (left, right) = self.terrain.channel(row);
        self.ship.0 = row;
        self.ship.1 = wanted.clamp(left, right);
        if self.terrain.solid(self.ship.0, self.ship.1) {
            // A column stands in the way: slide to the open cell beside it.
            self.shove_into_lane();
        }
    }

    /// Put a player shot in the air, slowed if the sector drags.
    fn launch(&mut self, mut shot: Shot) {
        if self.sector.drag() {
            shot.slow();
        }
        self.shots.push(shot);
    }

    /// Put an enemy shot in the air, slowed if the sector drags and heavier
    /// once the campaign is deep enough to warrant it.
    fn launch_enemy(&mut self, mut shot: Shot) {
        if self.sector.drag() {
            shot.slow();
        }
        if self.wave >= 15 {
            shot = shot.heavy();
        }
        self.enemy_shots.push(shot);
    }

    /// Fire the current gun, and every wing drone with it, if the cadence has
    /// come round again.
    pub fn fire(&mut self) {
        if self.status != Status::Playing || self.fire_cooldown > 0 || self.shots.len() >= MAX_SHOTS
        {
            return;
        }
        let cadence = self.cadence();
        self.fire_cooldown = if self.rapid > 0 {
            cadence.div_ceil(2)
        } else {
            cadence
        };
        let level = self.weapon_level;
        let dmg = self.gun_damage() + level as i32 - 1;
        let (r, c) = (self.ship.0 - 1, self.ship.1);
        match self.weapon {
            Weapon::Blaster => {
                let lanes: &[i16] = match level {
                    1 => &[0],
                    2 => &[-1, 1],
                    _ => &[-1, 0, 1],
                };
                for &dx in lanes {
                    self.launch(Shot::bolt((r, c + dx), 0, dmg + 1));
                }
            }
            Weapon::Spread => {
                let lanes: &[i16] = if level >= 2 {
                    &[-2, -1, 0, 1, 2]
                } else {
                    &[-1, 0, 1]
                };
                for &drift in lanes {
                    self.launch(Shot::bolt((r, c), drift, dmg));
                }
            }
            Weapon::Laser => {
                let lanes: &[i16] = match level {
                    1 => &[0],
                    2 => &[-1, 1],
                    _ => &[-1, 0, 1],
                };
                for &dx in lanes {
                    self.launch(Shot::beam((r, c + dx), dmg));
                }
            }
            Weapon::Homing => {
                let lanes: &[i16] = match level {
                    1 => &[0],
                    2 => &[-2, 2],
                    _ => &[-2, 0, 2],
                };
                for &dx in lanes {
                    self.launch(Shot::missile((r, c + dx), dmg + 1));
                }
            }
            Weapon::Plasma => {
                let half_width = if level >= 3 { 2 } else { 1 };
                self.launch(Shot::plasma((r, c), dmg + 2, half_width));
            }
            Weapon::Vulcan => {
                // The barrel walks a column either side as it hoses.
                let jitter = if self.tick.is_multiple_of(2) { 1 } else { -1 };
                self.launch(Shot::vulcan((r, c), 0, dmg));
                if level >= 2 {
                    self.launch(Shot::vulcan((r, c + jitter), 0, dmg));
                }
                if level >= 3 {
                    self.launch(Shot::vulcan((r, c - jitter), 0, dmg));
                }
            }
            Weapon::Rocket => {
                let lanes: &[i16] = match level {
                    1 => &[0],
                    2 => &[-2, 2],
                    _ => &[-2, 0, 2],
                };
                for &dx in lanes {
                    self.launch(Shot::rocket((r, c + dx), dmg + 2));
                }
            }
            Weapon::Flak => {
                self.launch(Shot::flak((r, c), dmg + 1, FLAK_FUSE));
                if level >= 2 {
                    self.launch(Shot::flak((r, c - 3), dmg + 1, FLAK_FUSE + 2));
                }
                if level >= 3 {
                    self.launch(Shot::flak((r, c + 3), dmg + 1, FLAK_FUSE + 2));
                }
            }
            Weapon::Rail => {
                self.launch(Shot::rail((r, c), dmg * 3 + 2));
                if level >= 3 {
                    for dx in [-2, 2] {
                        self.launch(Shot::rail((r, c + dx), dmg * 2));
                    }
                }
            }
            Weapon::Arc => {
                self.launch(Shot::arc((r, c), dmg + 1, level + 1));
            }
        }
        // The drones throw a plain bolt each, unless a surge has stunned them.
        if self.drone_stun == 0 {
            let drone_damage = self.gun_damage();
            for side in self.drones.clone() {
                let muzzle = (r.max(0), (c + side * DRONE_OFFSET).clamp(0, W - 1));
                self.launch(Shot::bolt(muzzle, 0, drone_damage));
            }
        }
    }

    /// Spend energy on the hull's special.
    pub fn special(&mut self) {
        let cost = self.special_cost();
        if self.status != Status::Playing || self.energy < cost {
            return;
        }
        self.energy -= cost;
        match self.class.special() {
            Special::Blink => {
                self.ship.1 = (self.ship.1 + self.facing * BLINK_DISTANCE).clamp(1, W - 2);
                self.invuln = self.invuln.max(BLINK_IFRAMES);
            }
            Special::Bulwark => self.bulwark = BULWARK_TICKS,
            Special::Barrage => {
                let damage = self.gun_damage() + 2;
                let row = self.ship.0 - 1;
                let mut col = 2;
                while col < W - 2 {
                    self.launch(Shot::bolt((row, col), 0, damage));
                    col += BARRAGE_STEP;
                }
            }
        }
    }

    /// Drop a smart bomb: every enemy shot and mine is wiped and everything on
    /// the court takes damage, the boss and its parts included.
    pub fn bomb(&mut self) {
        if self.status != Status::Playing || self.bombs == 0 {
            return;
        }
        self.bombs -= 1;
        self.flash = 6;
        self.enemy_shots.clear();
        self.mines.clear();
        let mut survivors = Vec::with_capacity(self.enemies.len());
        let mut payout = 0;
        for mut e in std::mem::take(&mut self.enemies) {
            e.hp -= BOMB_DAMAGE;
            if e.hp <= 0 {
                payout += e.kind.score();
            } else {
                survivors.push(e);
            }
        }
        self.enemies = survivors;
        self.asteroids.retain(|a| a.hp > BOMB_DAMAGE);
        self.debris.retain(|d| d.hp > BOMB_DAMAGE);
        for turret in self.turrets.iter_mut() {
            turret.hp -= BOMB_DAMAGE;
        }
        self.turrets.retain(|t| t.hp > 0);
        if let Some(boss) = self.boss.as_mut() {
            let bite = (boss.max_hp / 12).max(BOMB_DAMAGE);
            for part in boss.parts.iter_mut() {
                part.hp -= bite;
            }
            boss.parts.retain(|p| p.hp > 0);
            if !boss.armoured() {
                boss.hp -= bite;
            }
        }
        self.add_score(payout);
        self.gain_xp(payout);
        self.check_end();
    }

    /// Take a hit: the bulwark eats it, then shields, then a life goes and the
    /// gun drops a level. Either way the chain breaks and the hull flashes.
    fn damage_ship(&mut self, pips: u32) {
        if self.invuln > 0 || self.bulwark > 0 {
            return;
        }
        self.invuln = INVULN_TICKS;
        self.combo = 1;
        self.combo_timer = 0;
        let pips = pips.max(1);
        let soaked = self.shield.min(pips);
        self.shield -= soaked;
        if soaked == pips {
            return;
        }
        self.lives = self.lives.saturating_sub(1);
        self.shield = self.max_shield;
        self.weapon_level = self.weapon_level.saturating_sub(1).max(1);
        self.drones.pop();
    }

    /// Bank a kill at the current chain multiplier and extend the chain.
    fn award(&mut self, base: u32) {
        let points = base * self.combo;
        self.add_score(points);
        self.gain_xp(base);
        self.combo = (self.combo + 1).min(MAX_COMBO);
        self.combo_timer = COMBO_TICKS;
    }

    /// Pick up a dropped powerup.
    fn collect(&mut self, kind: PowerKind) {
        match kind {
            PowerKind::Gun(w) if w == self.weapon => {
                self.weapon_level = (self.weapon_level + 1).min(MAX_WEAPON_LEVEL);
            }
            PowerKind::Gun(w) => self.weapon = w,
            PowerKind::Shield => self.shield = (self.shield + 1).min(self.max_shield + 2),
            PowerKind::Bomb => self.bombs += 1,
            PowerKind::Rapid => self.rapid = RAPID_TICKS,
            PowerKind::Drone => {
                if self.drones.len() < MAX_DRONES {
                    let side = if self.drones.contains(&-1) { 1 } else { -1 };
                    self.drones.push(side);
                } else {
                    self.credits += MEDAL_SCORE / 2;
                }
            }
            PowerKind::Medal => {
                self.medals += 1;
                self.add_score(MEDAL_SCORE);
                self.credits += MEDAL_SCORE / 2;
            }
            PowerKind::Life => self.lives += 1,
        }
        self.add_score(25);
    }

    /// What a kill drops: guns half the time, then armour, rapid fire, a medal,
    /// a drone, a bomb and — rarely — a spare life.
    fn roll_power(&mut self) -> PowerKind {
        match self.rand() % 16 {
            0..=5 => {
                let i = (self.rand() % Weapon::ALL.len() as u64) as usize;
                PowerKind::Gun(Weapon::ALL[i])
            }
            6 | 7 => PowerKind::Shield,
            8 | 9 => PowerKind::Rapid,
            10 | 11 => PowerKind::Medal,
            12 => PowerKind::Drone,
            13 | 14 => PowerKind::Bomb,
            _ => PowerKind::Life,
        }
    }

    /// Age every timer by one tick: the chain cools, energy trickles back and
    /// the repair bay hands a shield pip over on its own cadence.
    fn tick_timers(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.fire_cooldown = self.fire_cooldown.saturating_sub(1);
        self.invuln = self.invuln.saturating_sub(1);
        self.rapid = self.rapid.saturating_sub(1);
        self.flash = self.flash.saturating_sub(1);
        self.bulwark = self.bulwark.saturating_sub(1);
        self.drone_stun = self.drone_stun.saturating_sub(1);
        self.banner = self.banner.saturating_sub(1);
        self.energy = (self.energy + self.regen()).min(self.max_energy());
        if self.combo_timer > 0 {
            self.combo_timer -= 1;
            if self.combo_timer == 0 {
                self.combo = 1;
            }
        }
        if self.loadout.has(Module::RepairBay) {
            self.repair_timer = self.repair_timer.saturating_sub(1);
            if self.repair_timer == 0 {
                self.repair_timer = REPAIR_CADENCE;
                self.shield = (self.shield + 1).min(self.max_shield);
            }
        }
    }

    /// Drift the formation one step on its cadence, reversing at the edges.
    fn sway(&mut self) {
        if self.sway_counter == 0 {
            let next = self.sway_x + self.sway_dir;
            if !(-SWAY_MAX..=SWAY_MAX).contains(&next) {
                self.sway_dir = -self.sway_dir;
            } else {
                self.sway_x = next;
            }
            self.sway_counter = SWAY_CADENCE;
        } else {
            self.sway_counter -= 1;
        }
    }

    /// Scroll the backdrop; the near layer moves every tick, the far one every
    /// third, which is what gives the court its parallax.
    fn advance_stars(&mut self) {
        let tick = self.tick;
        let mut wrapped = Vec::new();
        for star in self.stars.iter_mut() {
            let cadence = (STAR_LAYERS - star.layer) as u32;
            if tick.is_multiple_of(cadence) {
                star.pos.0 += 1;
                if star.pos.0 >= H {
                    star.pos.0 = 0;
                    wrapped.push(star.pos.1);
                }
            }
        }
        // Re-scatter whatever wrapped so the field never settles into columns.
        for _ in 0..wrapped.len() {
            let col = (self.rand() % W as u64) as i16;
            if let Some(star) = self.stars.iter_mut().find(|s| s.pos.0 == 0) {
                star.pos.1 = col;
            }
        }
    }

    /// Fire an ion surge if the sector is due one: it drains the reactor, stuns
    /// the drones and throws lightning down a few lanes.
    fn advance_storm(&mut self) {
        let cadence = self.sector.surge_cadence();
        if cadence == 0 || !self.tick.is_multiple_of(cadence) {
            return;
        }
        self.energy = self.energy.saturating_sub(SURGE_DRAIN);
        self.drone_stun = SURGE_STUN;
        self.flash = self.flash.max(3);
        for _ in 0..3 {
            let col = (self.rand() % (W as u64 - 4)) as i16 + 2;
            self.launch_enemy(Shot::enemy((0, col), 0, 2).heavy());
        }
    }

    /// Advance every enemy: hold formation, dive, or weave; shoot, telegraph or
    /// lay a mine on the way; ram the ship if it is in the way. A diver that
    /// runs off the bottom comes back round to its slot, but a kamikaze that
    /// misses is gone for good. Healers patch up whoever is flying near them.
    fn advance_enemies(&mut self) {
        let ship = self.ship;
        let sway = self.sway_x;
        let tick = self.tick;
        let healing = tick.is_multiple_of(HEAL_CADENCE);
        let healers: Vec<(i16, i16)> = self
            .enemies
            .iter()
            .filter(|e| e.kind == EnemyKind::Healer)
            .map(|e| e.pos)
            .collect();
        let mut spawned: Vec<Shot> = Vec::new();
        let mut mined: Vec<(i16, i16)> = Vec::new();
        let mut kept = Vec::with_capacity(self.enemies.len());
        let mut rammed = false;
        for mut e in std::mem::take(&mut self.enemies) {
            match e.state {
                EnemyState::Formation => {
                    e.pos = (e.home.0, e.home.1 + sway);
                    let dive = e.kind.dive_chance();
                    if dive > 0 && self.rand().is_multiple_of(self.odds(dive)) {
                        e.state = EnemyState::Diving { target_x: ship.1 };
                    } else if e.kind == EnemyKind::Weaver
                        && self.rand().is_multiple_of(self.odds(WEAVE_CHANCE))
                    {
                        let dir = if self.rand() & 1 == 0 { 1 } else { -1 };
                        e.state = EnemyState::Weaving { dir };
                    }
                }
                EnemyState::Diving { target_x } => {
                    e.pos.0 += e.kind.dive_speed();
                    e.pos.1 += (target_x - e.pos.1).signum();
                }
                EnemyState::Weaving { dir } => {
                    if tick.is_multiple_of(2) {
                        e.pos.0 += 1;
                    }
                    let dir = if (1..W - 1).contains(&(e.pos.1 + dir)) {
                        dir
                    } else {
                        -dir
                    };
                    e.pos.1 = (e.pos.1 + dir).clamp(1, W - 2);
                    e.state = EnemyState::Weaving { dir };
                }
            }
            let (left, right) = self.terrain.channel(e.pos.0);
            e.pos.1 = e.pos.1.clamp(left, right);
            if e.pos.0 >= H {
                if e.kind == EnemyKind::Kamikaze {
                    continue;
                }
                e.pos = (e.home.0, e.home.1 + sway);
                e.state = EnemyState::Formation;
            }
            if e.pos == ship {
                rammed = true;
                if e.kind == EnemyKind::Kamikaze {
                    continue;
                }
            }
            if healing && e.kind != EnemyKind::Healer && e.hp < e.max_hp {
                let mended = healers.iter().any(|h| {
                    (h.0 - e.pos.0).abs() <= HEAL_RANGE && (h.1 - e.pos.1).abs() <= HEAL_RANGE
                });
                if mended {
                    e.hp += 1;
                }
            }
            // A sniper spends its telegraph before the shot actually goes off.
            if e.charge > 0 {
                e.charge -= 1;
                if e.charge == 0 {
                    let drift = (ship.1 - e.pos.1).signum();
                    // A sniper round is heavy enough to cost two pips.
                    spawned.push(Shot::enemy((e.pos.0 + 1, e.pos.1), drift, 2).heavy());
                }
                kept.push(e);
                continue;
            }
            let fire = e.kind.fire_chance();
            if fire > 0 && self.rand().is_multiple_of(self.odds(fire)) {
                let muzzle = (e.pos.0 + 1, e.pos.1);
                match e.kind {
                    // Turrets lead the ship, bombers throw a three-way spread,
                    // snipers wind up first and miners leave a mine behind.
                    EnemyKind::Turret => {
                        spawned.push(Shot::enemy(muzzle, (ship.1 - e.pos.1).signum(), 1));
                    }
                    EnemyKind::Bomber => {
                        for drift in [-1, 0, 1] {
                            spawned.push(Shot::enemy(muzzle, drift, 1));
                        }
                    }
                    EnemyKind::Sniper => e.charge = SNIPER_CHARGE,
                    EnemyKind::Miner => mined.push(muzzle),
                    _ => spawned.push(Shot::enemy(muzzle, 0, 1)),
                }
            }
            kept.push(e);
        }
        self.enemies = kept;
        for shot in spawned {
            self.launch_enemy(shot);
        }
        for pos in mined {
            if pos.0 < H {
                self.mines.push(Mine {
                    pos,
                    fuse: MINE_FUSE,
                });
            }
        }
        if rammed {
            self.damage_ship(1);
        }
    }

    /// Sweep the boss across the top of the court and run its pattern for this
    /// kind and phase.
    fn advance_boss(&mut self) {
        let Some(mut boss) = self.boss.take() else {
            return;
        };
        boss.tick = boss.tick.wrapping_add(1);
        let ship = self.ship;
        let margin = boss.kind.core_half() + 1;
        let next = boss.pos.1 + boss.dir * boss.speed();
        if (margin..W - margin).contains(&next) {
            boss.pos.1 = next;
        } else {
            boss.dir = -boss.dir;
        }
        // The carrier also creeps down the court and back up as it launches.
        if boss.kind == BossKind::Carrier && boss.tick.is_multiple_of(24) {
            boss.pos.0 = FORMATION_TOP + (boss.tick / 24 % 3) as i16;
        }
        let cells = boss.part_cells();
        if boss.cooldown > 0 {
            boss.cooldown -= 1;
        } else {
            boss.cooldown = boss.cadence();
            let row = boss.pos.0 + boss.kind.core_depth() + 1;
            let aim = (ship.1 - boss.pos.1).signum();
            let phase = boss.phase();
            let mut volley: Vec<Shot> = Vec::new();
            match boss.kind {
                BossKind::Dreadnought => match phase {
                    1 => {
                        for dx in [-6, -2, 2, 6] {
                            volley.push(Shot::enemy((row, boss.pos.1 + dx), aim, 1));
                        }
                    }
                    2 => {
                        for drift in -2..=2 {
                            volley.push(Shot::enemy((row, boss.pos.1), drift, 1));
                        }
                    }
                    _ => {
                        for lane in [W / 6, W / 2, 5 * W / 6] {
                            volley.push(Shot::enemy((row, lane), 0, 2).heavy());
                        }
                        volley.push(Shot::enemy((row, boss.pos.1), aim, 2).heavy());
                    }
                },
                BossKind::Twin => {
                    // Every live turret throws an aimed shot; once the core is
                    // exposed it adds a fan of its own.
                    for &(r, c) in &cells {
                        volley.push(Shot::enemy((r + 1, c), (ship.1 - c).signum(), 1));
                    }
                    if phase >= 2 || cells.is_empty() {
                        for drift in [-2, 0, 2] {
                            volley.push(Shot::enemy((row, boss.pos.1), drift, 1));
                        }
                    }
                }
                BossKind::Carrier => {
                    // A curtain of shots across the hull, denser as it burns.
                    let step = if phase == 1 { 4 } else { 2 };
                    let mut dx = -6;
                    while dx <= 6 {
                        volley.push(Shot::enemy((row, boss.pos.1 + dx), 0, 1));
                        dx += step;
                    }
                }
                BossKind::Serpent => {
                    // The head spits at the ship; the body drools once enraged.
                    let head = Shot::enemy((row, boss.pos.1), aim, if phase == 3 { 2 } else { 1 });
                    volley.push(if phase == 3 { head.heavy() } else { head });
                    if phase >= 2 {
                        for &(r, c) in cells.iter().step_by(2) {
                            volley.push(Shot::enemy((r + 1, c), 0, 1));
                        }
                    }
                }
            }
            for shot in volley {
                self.launch_enemy(shot);
            }
        }
        // Carriers launch from their bays; the other hulls scramble escorts
        // once they are hurt.
        let launching = match boss.kind {
            BossKind::Carrier => !boss.parts.is_empty(),
            BossKind::Serpent => false,
            _ => boss.phase() >= 2,
        };
        if launching {
            if boss.minion_timer > 0 {
                boss.minion_timer -= 1;
            } else {
                boss.minion_timer = if boss.kind == BossKind::Carrier {
                    40
                } else {
                    80
                };
                let bays: Vec<(i16, i16)> = if boss.kind == BossKind::Carrier {
                    cells.clone()
                } else {
                    vec![
                        (boss.pos.0 + 3, boss.pos.1 - 6),
                        (boss.pos.0 + 3, boss.pos.1 + 6),
                    ]
                };
                for (r, c) in bays {
                    let home = (r.clamp(0, H - 2), c.clamp(1, W - 2));
                    let mut minion = self.hatch(EnemyKind::Kamikaze, home);
                    minion.state = EnemyState::Diving { target_x: ship.1 };
                    self.enemies.push(minion);
                }
            }
        }
        self.boss = Some(boss);
    }

    /// Column a homing missile should slide toward: the nearest enemy, or the
    /// boss when the formation is gone.
    fn steer(&self, shot: &Shot) -> i16 {
        let mut best: Option<(i32, i16)> = None;
        for e in &self.enemies {
            let d = (e.pos.0 - shot.pos.0).abs() as i32 + (e.pos.1 - shot.pos.1).abs() as i32;
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, (e.pos.1 - shot.pos.1).signum()));
            }
        }
        match (best, &self.boss) {
            (Some((_, drift)), _) => drift,
            (None, Some(boss)) => (boss.pos.1 - shot.pos.1).signum(),
            (None, None) => 0,
        }
    }

    /// Damage the boss under a shot: its parts shield it, and the twin's core
    /// cannot be touched at all until both turrets are gone.
    fn hit_boss(&mut self, shot: &Shot) -> bool {
        let Some(boss) = self.boss.as_mut() else {
            return false;
        };
        let (r, c) = shot.pos;
        let cells = boss.part_cells();
        let mut struck = false;
        for (i, &(pr, pc)) in cells.iter().enumerate() {
            if r == pr && (c - pc).abs() <= 1 + shot.half_width {
                boss.parts[i].hp -= shot.damage;
                struck = true;
                break;
            }
        }
        let before = boss.parts.len();
        boss.parts.retain(|p| p.hp > 0);
        let downed = (before - boss.parts.len()) as u32;
        let armoured = boss.armoured();
        let rows = boss.pos.0..=boss.pos.0 + boss.kind.core_depth();
        let half = boss.kind.core_half() + shot.half_width;
        if !struck && !armoured && rows.contains(&r) && (c - boss.pos.1).abs() <= half {
            boss.hp -= shot.damage;
            struck = true;
        }
        for _ in 0..downed {
            self.award(150);
        }
        struck
    }

    /// Chew on the hulk at a cell, if there is one; hulks stop shots dead.
    fn hit_debris(&mut self, pos: (i16, i16), damage: i32) -> bool {
        let Some(i) = self.debris.iter().position(|d| d.pos == pos) else {
            return false;
        };
        self.debris[i].hp -= damage;
        if self.debris[i].hp <= 0 {
            self.debris.remove(i);
            self.award(30);
        }
        true
    }

    /// Damage everything under a shot's footprint; returns whether it connected.
    fn hit_targets(&mut self, shot: &Shot) -> bool {
        let (r, c) = shot.pos;
        let mut hit = self.hit_boss(shot);
        if self.hit_debris((r, c), shot.damage) {
            hit = true;
        }
        if self.hit_turret((r, c), shot.damage) {
            hit = true;
        }
        let mut kills: Vec<(EnemyKind, (i16, i16))> = Vec::new();
        let mut kept = Vec::with_capacity(self.enemies.len());
        let reach_rows = shot.splash;
        let reach_cols = shot.half_width.max(shot.splash);
        for mut e in std::mem::take(&mut self.enemies) {
            if (e.pos.0 - r).abs() <= reach_rows && (e.pos.1 - c).abs() <= reach_cols {
                e.hp -= shot.damage;
                hit = true;
                if e.hp <= 0 {
                    kills.push((e.kind, e.pos));
                    continue;
                }
            }
            kept.push(e);
        }
        self.enemies = kept;
        // Rocks and mines are targets too; a mine that is shot goes off.
        let mut blasts = Vec::new();
        let mut rocks = Vec::with_capacity(self.asteroids.len());
        for mut a in std::mem::take(&mut self.asteroids) {
            if a.pos.0 == r && (a.pos.1 - c).abs() <= shot.half_width {
                a.hp -= shot.damage;
                hit = true;
                if a.hp <= 0 {
                    self.award(20);
                    continue;
                }
            }
            rocks.push(a);
        }
        self.asteroids = rocks;
        let mut mines = Vec::with_capacity(self.mines.len());
        for m in std::mem::take(&mut self.mines) {
            if m.pos.0 == r && (m.pos.1 - c).abs() <= shot.half_width {
                blasts.push(m.pos);
                hit = true;
                continue;
            }
            mines.push(m);
        }
        self.mines = mines;
        for pos in blasts {
            self.award(15);
            self.detonate(pos);
        }
        for (kind, pos) in kills {
            self.award(kind.score());
            // A splitter breaks into two grunts that dive straight at you.
            if kind == EnemyKind::Splitter {
                let target_x = self.ship.1;
                for dx in [-2, 2] {
                    let home = (pos.0, (pos.1 + dx).clamp(1, W - 2));
                    let mut half = self.hatch(EnemyKind::Grunt, home);
                    half.state = EnemyState::Diving { target_x };
                    self.enemies.push(half);
                }
            }
            if self.rand().is_multiple_of(self.difficulty.drop_chance()) {
                let kind = self.roll_power();
                self.powerups.push(Powerup { pos, kind });
            }
        }
        hit
    }

    /// Advance player shots a row at a time so nothing tunnels through a hull;
    /// only piercing beams survive a hit.
    fn advance_shots(&mut self) {
        let mut kept = Vec::with_capacity(self.shots.len());
        let mut fragments: Vec<Shot> = Vec::new();
        'shot: for mut s in std::mem::take(&mut self.shots) {
            if s.homing {
                s.drift = self.steer(&s);
            }
            for step in 0..s.speed.unsigned_abs() as i16 {
                s.pos.0 += s.speed.signum();
                if step == 0 {
                    s.pos.1 += s.drift;
                }
                if s.pos.0 < 0 || !(0..W).contains(&s.pos.1) {
                    continue 'shot;
                }
                if self.terrain.solid(s.pos.0, s.pos.1) {
                    self.terrain.carve(s.pos.0, s.pos.1);
                    continue 'shot;
                }
                if self.hit_targets(&s) {
                    // An arc bolt earths itself through the hulls around it.
                    while s.chain > 0 {
                        let Some(next) = self.nearest_enemy(s.pos) else {
                            break;
                        };
                        s.pos = next;
                        s.chain -= 1;
                        self.hit_targets(&s);
                    }
                    if !s.pierce {
                        continue 'shot;
                    }
                }
            }
            // A flak shell counts down and then throws its fan.
            if s.fuse > 0 {
                s.fuse -= 1;
                if s.fuse == 0 {
                    for drift in -2..=2 {
                        let mut fragment = Shot::bolt(s.pos, drift, s.damage);
                        fragment.speed = -1;
                        if self.sector.drag() {
                            fragment.slow();
                        }
                        fragments.push(fragment);
                    }
                    continue 'shot;
                }
            }
            kept.push(s);
        }
        self.shots = kept;
        self.shots.extend(fragments);
    }

    /// The nearest hull to a cell, for an arc bolt looking for its next jump.
    fn nearest_enemy(&self, from: (i16, i16)) -> Option<(i16, i16)> {
        self.enemies
            .iter()
            .map(|e| e.pos)
            .filter(|&p| p != from && (p.0 - from.0).abs() + (p.1 - from.1).abs() <= ARC_REACH)
            .min_by_key(|p| (p.0 - from.0).abs() + (p.1 - from.1).abs())
    }

    /// Advance enemy fire; hulks eat it, the bulwark eats it, and anything that
    /// gets past both and reaches the ship's cell is a hit.
    fn advance_enemy_shots(&mut self) {
        let ship = self.ship;
        let shielded = self.bulwark > 0;
        let mut kept = Vec::with_capacity(self.enemy_shots.len());
        let mut hits: Vec<u32> = Vec::new();
        let mut blocked: Vec<(i16, i16)> = Vec::new();
        'shot: for mut s in std::mem::take(&mut self.enemy_shots) {
            for step in 0..s.speed.unsigned_abs() as i16 {
                s.pos.0 += 1;
                if step == 0 {
                    s.pos.1 += s.drift;
                }
                if s.pos.0 >= H || !(0..W).contains(&s.pos.1) {
                    continue 'shot;
                }
                if self.debris.iter().any(|d| d.pos == s.pos) {
                    blocked.push(s.pos);
                    continue 'shot;
                }
                if self.terrain.solid(s.pos.0, s.pos.1) {
                    continue 'shot;
                }
                if s.pos == ship {
                    if !shielded {
                        hits.push(s.damage.max(1) as u32);
                    }
                    continue 'shot;
                }
            }
            kept.push(s);
        }
        self.enemy_shots = kept;
        for pos in blocked {
            self.hit_debris(pos, 1);
        }
        for pips in hits {
            self.damage_ship(pips);
        }
    }

    /// Tumble pickups down the court at half speed, pulled sideways by the
    /// magnet if one is fitted, and collect the ones the ship flies into.
    fn advance_powerups(&mut self) {
        let ship = self.ship;
        let falling = self.tick.is_multiple_of(2);
        let magnet = self.loadout.has(Module::Magnet);
        let mut kept = Vec::with_capacity(self.powerups.len());
        let mut taken = Vec::new();
        for mut p in std::mem::take(&mut self.powerups) {
            if falling {
                p.pos.0 += 1;
            }
            if magnet && (p.pos.0 - ship.0).abs() <= 8 {
                p.pos.1 += (ship.1 - p.pos.1).signum();
            }
            if p.pos.0 >= H {
                continue;
            }
            if (p.pos.0 - ship.0).abs() <= 1 && (p.pos.1 - ship.1).abs() <= 1 {
                taken.push(p.kind);
                continue;
            }
            kept.push(p);
        }
        self.powerups = kept;
        for kind in taken {
            self.collect(kind);
        }
    }

    /// Blow a mine: it throws a three-way spread down the court.
    fn detonate(&mut self, pos: (i16, i16)) {
        for drift in [-1, 0, 1] {
            self.launch_enemy(Shot::enemy(pos, drift, 1));
        }
        self.flash = self.flash.max(2);
    }

    /// Count mines down; one whose fuse runs out, or that the ship flies too
    /// close to, goes off.
    fn advance_mines(&mut self) {
        let ship = self.ship;
        let mut kept = Vec::with_capacity(self.mines.len());
        let mut blasts = Vec::new();
        for mut m in std::mem::take(&mut self.mines) {
            m.fuse = m.fuse.saturating_sub(1);
            let near = (m.pos.0 - ship.0).abs() <= MINE_TRIGGER
                && (m.pos.1 - ship.1).abs() <= MINE_TRIGGER;
            if m.fuse == 0 || near {
                blasts.push(m.pos);
                continue;
            }
            kept.push(m);
        }
        self.mines = kept;
        for pos in blasts {
            self.detonate(pos);
        }
    }

    /// Drift rocks down the court, spawning them at the sector's rate; flying
    /// into one costs the ship and breaks the rock.
    fn advance_asteroids(&mut self) {
        let chance = self.odds(self.sector.asteroid_chance());
        if self.rand().is_multiple_of(chance) {
            let col = (self.rand() % (W as u64 - 4)) as i16 + 2;
            let drift = (self.rand() % 3) as i16 - 1;
            self.asteroids.push(Asteroid {
                pos: (0, col),
                hp: ASTEROID_HP + self.wave_armour(),
                drift,
            });
        }
        let ship = self.ship;
        let falling = self.tick.is_multiple_of(2);
        let mut kept = Vec::with_capacity(self.asteroids.len());
        let mut rammed = false;
        for mut a in std::mem::take(&mut self.asteroids) {
            if falling {
                a.pos.0 += 1;
                a.pos.1 += a.drift;
                if !(1..W - 1).contains(&a.pos.1) {
                    a.drift = -a.drift;
                    a.pos.1 = a.pos.1.clamp(1, W - 2);
                }
            }
            if a.pos.0 >= H {
                continue;
            }
            if a.pos == ship {
                rammed = true;
                continue;
            }
            kept.push(a);
        }
        self.asteroids = kept;
        if rammed {
            self.damage_ship(1);
        }
    }

    /// Creep the hulks down the lane; one that reaches the ship breaks on it.
    fn advance_debris(&mut self) {
        if !self.tick.is_multiple_of(3) {
            return;
        }
        let ship = self.ship;
        let mut kept = Vec::with_capacity(self.debris.len());
        let mut rammed = false;
        for mut d in std::mem::take(&mut self.debris) {
            d.pos.0 += 1;
            if d.pos.0 >= H {
                continue;
            }
            if d.pos == ship {
                rammed = true;
                continue;
            }
            kept.push(d);
        }
        self.debris = kept;
        if rammed {
            // A hulk is a great deal heavier than a rock.
            self.damage_ship(2);
        }
    }

    /// Settle the round: a dead boss pays a bounty, no lives ends it, and an
    /// empty court starts the countdown into the hangar.
    fn check_end(&mut self) {
        if self
            .boss
            .as_ref()
            .is_some_and(|b| b.hp <= 0 && !b.armoured())
        {
            let bounty = 500 * self.wave;
            self.boss = None;
            self.add_score(bounty);
            self.gain_xp(bounty / 2);
        }
        if self.lives == 0 {
            self.status = Status::Lost;
            return;
        }
        if self.enemies.is_empty() && self.boss.is_none() && self.status == Status::Playing {
            self.status = Status::WaveClear;
            self.intermission = INTERMISSION_TICKS;
            self.route = self.roll_route();
            if let Some(&first) = self.route.first() {
                self.node = first;
            }
            let bonus = 100 * self.wave;
            self.add_score(bonus);
            self.credits += bonus;
        }
    }

    /// What a hangar line costs here: prices climb with the campaign, so late
    /// salvage never turns the store into a rubber stamp.
    pub fn priced(&self, base: u32) -> u32 {
        base * (100 + 12 * self.wave) / 100
    }

    /// The hangar's stock, in the order it is listed and keyed.
    pub fn shop_lines(&self) -> Vec<ShopLine> {
        let mut lines = Vec::new();
        let mut keys = SHOP_KEYS.iter();
        let push = |lines: &mut Vec<ShopLine>,
                    key: Option<&char>,
                    entry: ShopEntry,
                    label: String,
                    detail: &'static str,
                    price: u32,
                    available: bool| {
            if let Some(&key) = key {
                lines.push(ShopLine {
                    key,
                    entry,
                    label,
                    detail,
                    price,
                    available,
                });
            }
        };
        for part in Part::ALL {
            let tier = self.loadout.tier(part);
            let maxed = tier >= MAX_TIER;
            let price = self.priced(part.price(tier));
            let label = if maxed {
                format!("{} (tier {tier}, maxed)", part.name())
            } else {
                format!("{} tier {} → {}", part.name(), tier, tier + 1)
            };
            push(
                &mut lines,
                keys.next(),
                ShopEntry::Component(part),
                label,
                part.detail(),
                price,
                !maxed && self.credits >= price,
            );
        }
        for module in Module::ALL {
            if self.loadout.has(module) {
                continue;
            }
            let price = self.priced(module.price());
            push(
                &mut lines,
                keys.next(),
                ShopEntry::Fitting(module),
                module.name().to_string(),
                module.detail(),
                price,
                self.credits >= price,
            );
        }
        for stock in Stock::ALL {
            let price = self.priced(stock.price());
            let usable = match stock {
                Stock::Repair => self.shield < self.max_shield,
                Stock::GunLevel => self.weapon_level < MAX_WEAPON_LEVEL,
                Stock::Drone => self.drones.len() < MAX_DRONES,
                _ => true,
            };
            let label = match stock {
                Stock::GunSwap => format!("swap gun → {}", self.weapon.next().name()),
                _ => stock.name().to_string(),
            };
            push(
                &mut lines,
                keys.next(),
                ShopEntry::Consumable(stock),
                label,
                stock.detail(),
                price,
                usable && self.credits >= price,
            );
        }
        lines
    }

    /// Buy the hangar line under `key`; returns whether the sale went through.
    pub fn buy(&mut self, key: char) -> bool {
        if self.status != Status::Hangar {
            return false;
        }
        let Some(line) = self.shop_lines().into_iter().find(|l| l.key == key) else {
            return false;
        };
        if !line.available {
            return false;
        }
        self.credits -= line.price;
        match line.entry {
            ShopEntry::Component(part) => {
                self.loadout.upgrade(part);
                if part == Part::Plating {
                    self.recompute_shield();
                    self.shield += 1;
                }
                if part == Part::Reactor {
                    self.energy = self.max_energy();
                }
            }
            ShopEntry::Fitting(module) => self.loadout.modules.push(module),
            ShopEntry::Consumable(stock) => match stock {
                Stock::Repair => self.shield = self.max_shield,
                Stock::GunLevel => {
                    self.weapon_level = (self.weapon_level + 1).min(MAX_WEAPON_LEVEL)
                }
                Stock::GunSwap => self.weapon = self.weapon.next(),
                Stock::Drone => {
                    let side = if self.drones.contains(&-1) { 1 } else { -1 };
                    self.drones.push(side);
                }
                Stock::Bomb => self.bombs += 1,
                Stock::Rapid => self.rapid = RAPID_TICKS,
                Stock::Life => self.lives += 1,
            },
        }
        true
    }

    /// True while this leg of the route is a danger run.
    pub fn danger(&self) -> bool {
        self.node.bonus == NodeBonus::Danger
    }

    /// Squeeze a formation slot into the rock: the wave keeps its shape, but a
    /// narrow channel packs it in rather than burying half of it in the wall.
    fn place(&self, slot: (i16, i16)) -> (i16, i16) {
        let (left, right) = self.terrain.channel(slot.0);
        let span = (right - left).max(1);
        let x = left + (slot.1 - 1) * span / (W - 2);
        (slot.0, x.clamp(left, right))
    }

    /// Lay the rock, bolt turrets to it and set whatever the sector throws at
    /// you on top of the formation.
    fn dress_terrain(&mut self) {
        let seed = self.rand();
        self.terrain = Terrain::new(self.node.terrain, seed);
        self.turrets.clear();
        self.hazards.clear();
        if self.node.terrain != TerrainKind::Open {
            for _ in 0..TURRETS_PER_MAP {
                if let Some(turret) = self.turret_on_the_rock() {
                    self.turrets.push(turret);
                }
            }
        }
        match self.sector {
            Sector::Nebula => {
                let col = (self.rand() % (W as u64 - 8)) as i16 + 4;
                let row = (self.rand() % 6) as i16 + 3;
                self.hazards.push(Hazard::GravityWell { pos: (row, col) });
            }
            Sector::IonStorm => {
                let push = if self.rand() & 1 == 0 { 1 } else { -1 };
                self.hazards.push(Hazard::IonStream { push });
            }
            Sector::SolarCorona => {
                for dir in [1, -1] {
                    let col = (self.rand() % (W as u64 - 4)) as i16 + 2;
                    self.hazards.push(Hazard::SolarFlare { col, dir });
                }
            }
            Sector::CometTrail => {
                let push = if self.rand() & 1 == 0 { 1 } else { -1 };
                self.hazards.push(Hazard::IonStream { push });
            }
            Sector::VoidRift => {
                for _ in 0..2 {
                    let col = (self.rand() % (W as u64 - 8)) as i16 + 4;
                    let row = (self.rand() % 8) as i16 + 3;
                    self.hazards.push(Hazard::GravityWell { pos: (row, col) });
                }
            }
            _ => {}
        }
    }

    /// Find a cell of rock with open court under it and put a gun on it.
    fn turret_on_the_rock(&mut self) -> Option<WallTurret> {
        for _ in 0..12 {
            let row = (self.rand() % (SHIP_TOP as u64 - 2)) as i16 + 1;
            let (left, right) = self.terrain.channel(row);
            let col = if self.rand() & 1 == 0 {
                left - 1
            } else {
                right + 1
            };
            if (1..W - 1).contains(&col) && self.terrain.solid(row, col) {
                return Some(WallTurret {
                    pos: (row, col),
                    hp: TURRET_HP + self.wave_armour(),
                    cooldown: TURRET_CADENCE,
                });
            }
        }
        None
    }

    /// Scroll the rock, ride the turrets down with it and let them shoot; the
    /// hull scrapes if it is caught in the wall.
    fn advance_terrain(&mut self) {
        if self.node.terrain != TerrainKind::Open && self.tick.is_multiple_of(SCROLL_CADENCE) {
            self.terrain.scroll();
            let mut kept = Vec::with_capacity(self.turrets.len());
            for mut t in std::mem::take(&mut self.turrets) {
                t.pos.0 += 1;
                if t.pos.0 < H {
                    kept.push(t);
                }
            }
            self.turrets = kept;
            if self.turrets.len() < TURRETS_PER_MAP {
                if let Some(turret) = self.turret_on_the_rock() {
                    self.turrets.push(turret);
                }
            }
        }
        // Turret fire, aimed down the court at the hull.
        let ship = self.ship;
        let mut volley = Vec::new();
        for t in self.turrets.iter_mut() {
            if t.cooldown > 0 {
                t.cooldown -= 1;
                continue;
            }
            t.cooldown = TURRET_CADENCE;
            volley.push(Shot::enemy(
                (t.pos.0 + 1, t.pos.1),
                (ship.1 - t.pos.1).signum(),
                1,
            ));
        }
        for shot in volley {
            self.launch_enemy(shot);
        }
        // Rock closing on the hull shoves it aside; it only crushes when there
        // is nowhere left to go.
        self.shove_into_lane();
    }

    /// Push the hull to the nearest open cell on its row. Flying is never
    /// punished by rock arriving from above — only by rock with no way out.
    fn shove_into_lane(&mut self) {
        let row = self.ship.0;
        if !self.terrain.solid(row, self.ship.1) {
            return;
        }
        let (left, right) = self.terrain.channel(row);
        for step in 1..W {
            for col in [self.ship.1 - step, self.ship.1 + step] {
                if (left..=right).contains(&col) && !self.terrain.solid(row, col) {
                    self.ship.1 = col;
                    return;
                }
            }
        }
        self.damage_ship(1);
    }

    /// Run whatever the sector does to the hull beyond the rock and the wave.
    fn advance_hazards(&mut self) {
        let tick = self.tick;
        let ship = self.ship;
        let mut pull = 0;
        let mut burn = false;
        let mut updated = Vec::with_capacity(self.hazards.len());
        for hazard in std::mem::take(&mut self.hazards) {
            match hazard {
                Hazard::GravityWell { pos } => {
                    if tick.is_multiple_of(2) {
                        pull += (pos.1 - ship.1).signum();
                    }
                    updated.push(hazard);
                }
                Hazard::IonStream { push } => {
                    if tick.is_multiple_of(3) {
                        pull += push;
                    }
                    updated.push(hazard);
                }
                Hazard::SolarFlare { col, dir } => {
                    let (col, dir) = if tick.is_multiple_of(FLARE_CADENCE) {
                        let next = col + dir;
                        if (1..W - 1).contains(&next) {
                            (next, dir)
                        } else {
                            (col - dir, -dir)
                        }
                    } else {
                        (col, dir)
                    };
                    if ship.1 == col && tick % FLARE_PERIOD < FLARE_ACTIVE {
                        burn = true;
                    }
                    updated.push(Hazard::SolarFlare { col, dir });
                }
            }
        }
        self.hazards = updated;
        if pull != 0 {
            let (left, right) = self.terrain.channel(self.ship.0);
            self.ship.1 = (self.ship.1 + pull).clamp(left.max(1), right.min(W - 2));
        }
        if burn {
            self.damage_ship(1);
        }
    }

    /// Chew on the wall turret at a cell, if there is one.
    fn hit_turret(&mut self, pos: (i16, i16), damage: i32) -> bool {
        let Some(i) = self.turrets.iter().position(|t| t.pos == pos) else {
            return false;
        };
        self.turrets[i].hp -= damage;
        if self.turrets[i].hp <= 0 {
            self.turrets.remove(i);
            self.award(60);
        }
        true
    }

    /// The three stops the star chart offers from here.
    fn roll_route(&mut self) -> Vec<RouteNode> {
        let mut nodes = Vec::with_capacity(ROUTE_CHOICES);
        for _ in 0..ROUTE_CHOICES {
            let sector = Sector::ALL[(self.rand() % Sector::ALL.len() as u64) as usize];
            let terrain = TerrainKind::ALL[(self.rand() % TerrainKind::ALL.len() as u64) as usize];
            let bonus = match self.rand() % 4 {
                0 => NodeBonus::Cache(200 + 60 * self.wave),
                1 => NodeBonus::Armoury(Weapon::ALL[(self.rand() % 5) as usize]),
                2 => NodeBonus::Refit,
                _ => NodeBonus::Danger,
            };
            nodes.push(RouteNode {
                sector,
                terrain,
                bonus,
            });
        }
        nodes
    }

    /// Pick the next stop on the chart; returns whether the index was on it.
    pub fn choose_route(&mut self, index: usize) -> bool {
        if self.status != Status::Hangar {
            return false;
        }
        let Some(&node) = self.route.get(index) else {
            return false;
        };
        self.node = node;
        true
    }

    /// Hand over whatever the chosen stop was carrying.
    fn claim_node_bonus(&mut self) {
        match self.node.bonus {
            NodeBonus::Cache(credits) => self.credits += credits,
            NodeBonus::Armoury(weapon) => {
                self.weapon = weapon;
                self.weapon_level = self.weapon_level.max(2);
            }
            NodeBonus::Refit => {
                self.shield = self.max_shield;
                self.bombs += 1;
            }
            // A danger run pays for itself through wave_armour and the salvage
            // multiplier rather than up front.
            NodeBonus::Danger => {}
        }
    }

    /// Advance one tick of the round, or of the pause before the hangar opens.
    pub fn step(&mut self) {
        match self.status {
            Status::Playing => {
                self.tick_timers();
                self.advance_stars();
                self.advance_terrain();
                self.advance_hazards();
                self.advance_storm();
                self.sway();
                self.advance_enemies();
                self.advance_boss();
                self.advance_shots();
                self.advance_enemy_shots();
                self.advance_mines();
                self.advance_asteroids();
                self.advance_debris();
                self.advance_powerups();
                self.check_end();
            }
            Status::WaveClear => {
                self.intermission = self.intermission.saturating_sub(1);
                if self.intermission == 0 {
                    self.status = Status::Hangar;
                }
            }
            _ => {}
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Nova overlay.
pub struct Nova {
    game: Game,
    seed: u64,
    /// Highlighted hull on the picker.
    pick: usize,
    /// Highlighted difficulty on the picker.
    diff: usize,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
    /// Rendered frames, used only to blink the hull while it is invulnerable.
    frames: u64,
}

impl Nova {
    pub fn new() -> Self {
        Nova {
            game: Game::new(1),
            seed: 1,
            pick: 1,
            diff: 0,
            paused: false,
            last: None,
            interval: Duration::from_millis(70),
            frames: 0,
        }
    }

    /// Back to the hull picker with a fresh seed.
    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
        self.paused = false;
        self.last = None;
    }

    /// Straight back into a round with the same hull and difficulty.
    fn retry(&mut self) {
        let (class, difficulty) = (self.game.class, self.game.difficulty);
        self.restart();
        self.game.start(class, difficulty);
    }

    /// Launch the highlighted hull at the highlighted difficulty.
    fn launch(&mut self) {
        let class = ShipClass::ALL[self.pick];
        let difficulty = Difficulty::ALL[self.diff];
        self.game.start(class, difficulty);
    }

    /// Running = a live round or the cleared-wave pause, and not paused.
    fn running(&self) -> bool {
        matches!(self.game.status, Status::Playing | Status::WaveClear) && !self.paused
    }

    /// The hull picker, shown before the first wave.
    fn render_select(&self, area: Rect, surface: &mut Surface, ctx: &Context) {
        let theme = &ctx.editor.theme;
        let header = theme.get("ui.text.focus");
        let text = theme.get("ui.text");
        let dim = theme.get("ui.linenr");
        let ox = area.x + 2;
        let mut y = area.y + 1;
        surface.set_string(ox, y, "N O V A", header);
        y += 2;
        surface.set_string(
            ox,
            y,
            "Pick a hull — ←/→ or 1/2/3, d cycles difficulty, Enter launches, q quits.",
            text,
        );
        y += 2;
        for (i, class) in ShipClass::ALL.iter().enumerate() {
            let marker = if i == self.pick { "▶" } else { " " };
            let style = if i == self.pick { header } else { text };
            surface.set_string(
                ox,
                y,
                &format!("{marker} {}  {}  {}", i + 1, class.glyph(), class.name()),
                style,
            );
            surface.set_string(
                ox + 22,
                y,
                &format!(
                    "shield {}  dmg {}  cadence {}  bombs {}  {}",
                    class.max_shield(),
                    class.damage(),
                    class.fire_cadence(),
                    class.bombs(),
                    class.special().name()
                ),
                dim,
            );
            surface.set_string(ox + 4, y + 1, class.blurb(), dim);
            y += 3;
        }
        let difficulty = Difficulty::ALL[self.diff];
        surface.set_string(
            ox,
            y,
            &format!(
                "Difficulty  [{}]   +{} armour on every enemy, ×{} score",
                difficulty.name(),
                difficulty.armour(),
                difficulty.score_bonus()
            ),
            header,
        );
        y += 2;
        surface.set_string(
            ox,
            y,
            "Build the ship in the hangar between waves: engine, reactor, plating,",
            dim,
        );
        surface.set_string(
            ox,
            y + 1,
            "cannon, magazine, plus magnet, autoloader, salvager, repair bay, overdrive.",
            dim,
        );
        surface.set_string(
            ox,
            y + 2,
            "Ten sectors over nine kinds of rock, picked from the star chart each hangar.",
            dim,
        );
        surface.set_string(
            ox,
            y + 3,
            "Every 4th wave is a boss: dreadnought, twin, carrier, serpent.",
            dim,
        );
    }

    /// The hangar, where salvage turns into a better ship.
    fn render_hangar(&self, area: Rect, surface: &mut Surface, ctx: &Context) {
        let theme = &ctx.editor.theme;
        let header = theme.get("ui.text.focus");
        let text = theme.get("ui.text");
        let dim = theme.get("ui.linenr");
        let g = &self.game;
        let ox = area.x + 2;
        let mut y = area.y;
        surface.set_string(
            ox,
            y,
            &format!(
                "HANGAR — wave {} cleared.  salvage {}   score {}",
                g.wave, g.credits, g.score
            ),
            header,
        );
        y += 1;
        surface.set_string(
            ox,
            y,
            &format!(
                "pilot level {} ({}/{} xp)   next: {}   hull {} · {}",
                g.level,
                g.xp,
                g.xp_next,
                LevelReward::of_level(g.level + 1).name(),
                g.class.name(),
                g.difficulty.name()
            ),
            text,
        );
        y += 1;
        let fitted: Vec<&str> = g.loadout.modules.iter().map(|m| m.name()).collect();
        surface.set_string(
            ox,
            y,
            &format!(
                "thrust {}  cadence {}  damage {}  shield {}/{}  regen {}  modules: {}",
                g.thrust(),
                g.cadence(),
                g.gun_damage(),
                g.shield,
                g.max_shield,
                g.regen(),
                if fitted.is_empty() {
                    "none".to_string()
                } else {
                    fitted.join(", ")
                }
            ),
            dim,
        );
        y += 2;
        for line in g.shop_lines() {
            let style = if line.available { text } else { dim };
            surface.set_string(
                ox,
                y,
                &format!("[{}] {:<28} {:>5}", line.key, line.label, line.price),
                style,
            );
            surface.set_string(ox + 40, y, line.detail, dim);
            y += 1;
        }
        y += 1;
        surface.set_string(ox, y, "STAR CHART — pick the next stop:", header);
        y += 1;
        for (i, node) in g.route.iter().enumerate() {
            let key = ROUTE_KEYS[i.min(ROUTE_KEYS.len() - 1)];
            let chosen = *node == g.node;
            surface.set_string(
                ox,
                y,
                &format!(
                    "{} [{}] {}",
                    if chosen { "▶" } else { " " },
                    key,
                    node.label()
                ),
                if chosen { header } else { text },
            );
            surface.set_string(ox + 52, y, node.terrain.blurb(), dim);
            y += 1;
        }
        y += 1;
        surface.set_string(
            ox,
            y,
            &format!(
                "Next up: wave {} — {} ({}).  Enter launches · q quits.",
                g.wave + 1,
                g.node.sector.name(),
                g.node.sector.blurb()
            ),
            header,
        );
    }
}

impl Default for Nova {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Nova {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        if matches!(key, key!('q') | key!(Esc) | ctrl!('c')) {
            return EventResult::Consumed(Some(close));
        }
        match self.game.status {
            Status::Select => {
                let hulls = ShipClass::ALL.len();
                match key {
                    key!(Left) | key!('h') => self.pick = (self.pick + hulls - 1) % hulls,
                    key!(Right) | key!('l') => self.pick = (self.pick + 1) % hulls,
                    key!('d') | key!(Tab) => self.diff = (self.diff + 1) % Difficulty::ALL.len(),
                    key!('1') => {
                        self.pick = 0;
                        self.launch();
                    }
                    key!('2') => {
                        self.pick = 1;
                        self.launch();
                    }
                    key!('3') => {
                        self.pick = 2;
                        self.launch();
                    }
                    key!(Enter) | key!(' ') => self.launch(),
                    _ => {}
                }
            }
            Status::Hangar => match key {
                key!(Enter) => self.game.launch_next_wave(),
                key!('n') => self.restart(),
                key!('z') => {
                    self.game.choose_route(0);
                }
                key!('x') => {
                    self.game.choose_route(1);
                }
                key!('v') => {
                    self.game.choose_route(2);
                }
                _ => {
                    // Everything else is a hangar line key.
                    if let Some(c) = key.char() {
                        self.game.buy(c);
                    }
                }
            },
            _ => match key {
                key!(Left) | key!('h') => self.game.move_ship(-1, 0),
                key!(Right) | key!('l') => self.game.move_ship(1, 0),
                key!(Up) | key!('k') => self.game.move_ship(0, -1),
                key!(Down) | key!('j') => self.game.move_ship(0, 1),
                key!(' ') | key!('f') => self.game.fire(),
                key!('x') => self.game.special(),
                key!('b') => self.game.bomb(),
                key!('p') => self.paused = !self.paused,
                key!('r') => self.retry(),
                key!('n') => self.restart(),
                _ => {}
            },
        }
        if self.running() {
            if self.last.is_none() {
                self.last = Some(Instant::now());
            }
            zmax_event::request_redraw();
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        // Advance on wall-clock delta, then schedule the next frame while running.
        let now = Instant::now();
        if self.running() {
            match self.last {
                Some(t) if now.duration_since(t) >= self.interval => {
                    self.game.step();
                    self.last = Some(now);
                }
                None => self.last = Some(now),
                _ => {}
            }
            zmax_event::request_redraw();
        }
        self.frames = self.frames.wrapping_add(1);

        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let wall_style = theme.get("ui.linenr");
        let star_style = theme.get("ui.linenr");
        let enemy_style = theme.get("error");
        let tank_style = theme.get("warning");
        let boss_style = theme.get("error");
        let part_style = theme.get("warning");
        let ship_style = theme.get("function");
        let drone_style = theme.get("constant");
        let shot_style = theme.get("warning");
        let beam_style = theme.get("ui.text.focus");
        let eshot_style = theme.get("error");
        let power_style = theme.get("string");
        let hazard_style = theme.get("comment");
        let rock_style = theme.get("ui.linenr");

        surface.clear_with(area, bg);
        if area.width < W as u16 + 4 || area.height < H as u16 + 7 {
            surface.set_string(
                area.x,
                area.y,
                &format!("Nova needs a {}×{} window.", W + 4, H + 7),
                text_style,
            );
            return;
        }
        match self.game.status {
            Status::Select => {
                self.render_select(area, surface, ctx);
                return;
            }
            Status::Hangar => {
                self.render_hangar(area, surface, ctx);
                return;
            }
            _ => {}
        }

        let g = &self.game;
        let ox = area.x + 2;
        let top = area.y + 4;
        surface.set_string(
            ox,
            area.y,
            &format!(
                "NOVA  wave {}  {}  score {}  chain ×{}",
                g.wave,
                match &g.boss {
                    Some(boss) => format!(
                        "{} · {} · {}",
                        g.sector.name(),
                        g.node.terrain.name(),
                        boss.kind.name()
                    ),
                    None => format!(
                        "{} · {} · {}",
                        g.sector.name(),
                        g.node.terrain.name(),
                        g.formation.name()
                    ),
                },
                g.score,
                g.combo
            ),
            header_style,
        );
        let shields = "▮".repeat(g.shield as usize);
        let spent = "▯".repeat(g.max_shield.saturating_sub(g.shield) as usize);
        surface.set_string(
            ox,
            area.y + 1,
            &format!(
                "GUN {} L{}   SHIELD {}{}   BOMB {}   LIVES {}{}",
                g.weapon.name(),
                g.weapon_level,
                shields,
                spent,
                "◆".repeat(g.bombs as usize),
                "♥".repeat(g.lives as usize),
                if g.rapid_active() { "   RAPID" } else { "" }
            ),
            text_style,
        );
        let pips = (g.energy * 10 / g.max_energy().max(1)).min(10) as usize;
        surface.set_string(
            ox,
            area.y + 2,
            &format!(
                "ENERGY {}{} {}{}   DRONES {}{}   MEDALS {}",
                "▰".repeat(pips),
                "▱".repeat(10 - pips),
                g.class.special().name(),
                if g.special_ready() { " READY" } else { "" },
                "◇".repeat(g.drones.len()),
                if g.drone_stun > 0 { " STUNNED" } else { "" },
                g.medals
            ),
            if g.special_ready() {
                header_style
            } else {
                text_style
            },
        );
        // The boss bar takes the fourth line while a boss is up; otherwise the
        // pilot's progress does.
        match &g.boss {
            Some(boss) => {
                let width = 24i32;
                let filled = (boss.hp.max(0) * width / boss.max_hp.max(1)) as usize;
                surface.set_string(
                    ox,
                    area.y + 3,
                    &format!(
                        "{} {}{}  phase {}{}",
                        boss.kind.name().to_uppercase(),
                        "▰".repeat(filled),
                        "▱".repeat(width as usize - filled),
                        boss.phase(),
                        match boss.parts.len() {
                            0 => String::new(),
                            n => format!("  parts {n}"),
                        }
                    ),
                    boss_style,
                );
            }
            None => {
                let bar = (g.xp * 10 / g.xp_next.max(1)).min(10) as usize;
                surface.set_string(
                    ox,
                    area.y + 3,
                    &format!(
                        "LVL {}  XP {}{}  SALVAGE {}   E{} R{} P{} C{} M{}",
                        g.level,
                        "▰".repeat(bar),
                        "▱".repeat(10 - bar),
                        g.credits,
                        g.loadout.tier(Part::Engine),
                        g.loadout.tier(Part::Reactor),
                        g.loadout.tier(Part::Plating),
                        g.loadout.tier(Part::Cannon),
                        g.loadout.tier(Part::Magazine)
                    ),
                    text_style,
                );
            }
        }

        // Court walls.
        for c in 0..W {
            surface.set_string(ox + c as u16, top, "─", wall_style);
            surface.set_string(ox + c as u16, top + 1 + H as u16, "─", wall_style);
        }
        let cell = |r: i16, c: i16| (ox + c as u16, top + 1 + r as u16);
        let on_board = |r: i16, c: i16| (0..H).contains(&r) && (0..W).contains(&c);

        // The sector backdrop, drawn first so everything else sits on top.
        for star in &g.stars {
            let (r, c) = star.pos;
            if on_board(r, c) {
                let (x, y) = cell(r, c);
                surface.set_string(x, y, g.sector.star_glyph(star.layer), star_style);
            }
        }
        // The rock itself: everything outside the flyable channel.
        if g.node.terrain != TerrainKind::Open {
            for r in 0..H {
                for c in 0..W {
                    if g.terrain.solid(r, c) {
                        let (x, y) = cell(r, c);
                        surface.set_string(x, y, "▓", rock_style);
                    }
                }
            }
        }
        for t in &g.turrets {
            let (r, c) = t.pos;
            if on_board(r, c) {
                let (x, y) = cell(r, c);
                surface.set_string(x, y, "Ø", enemy_style);
            }
        }
        for hazard in &g.hazards {
            match *hazard {
                Hazard::GravityWell { pos } => {
                    for (dr, dc) in [(0, 0), (0, -1), (0, 1), (-1, 0), (1, 0)] {
                        let (r, c) = (pos.0 + dr, pos.1 + dc);
                        if on_board(r, c) {
                            let (x, y) = cell(r, c);
                            let glyph = if (dr, dc) == (0, 0) { "◎" } else { "∙" };
                            surface.set_string(x, y, glyph, beam_style);
                        }
                    }
                }
                Hazard::SolarFlare { col, .. } => {
                    // Dim while it is cold, lit while it burns.
                    let hot = g.tick % FLARE_PERIOD < FLARE_ACTIVE;
                    let (glyph, style) = if hot {
                        ("≋", shot_style)
                    } else {
                        ("┊", hazard_style)
                    };
                    for r in 0..H {
                        if on_board(r, col) {
                            let (x, y) = cell(r, col);
                            surface.set_string(x, y, glyph, style);
                        }
                    }
                }
                Hazard::IonStream { push } => {
                    let glyph = if push > 0 { "»" } else { "«" };
                    for r in (0..H).step_by(4) {
                        for c in [1, W - 2] {
                            if on_board(r, c) {
                                let (x, y) = cell(r, c);
                                surface.set_string(x, y, glyph, beam_style);
                            }
                        }
                    }
                }
            }
        }
        // The boss hull and its parts.
        if let Some(boss) = &g.boss {
            let half = boss.kind.core_half();
            for dx in -half..=half {
                let (r, c) = (boss.pos.0, boss.pos.1 + dx);
                if on_board(r, c) {
                    let (x, y) = cell(r, c);
                    let glyph = if dx == 0 { "◉" } else { "▓" };
                    surface.set_string(x, y, glyph, boss_style);
                }
                if boss.kind.core_depth() > 0 && dx.abs() <= 2 && on_board(boss.pos.0 + 1, c) {
                    let (x, y) = cell(boss.pos.0 + 1, c);
                    surface.set_string(x, y, "▀", boss_style);
                }
            }
            for (r, c) in boss.part_cells() {
                for dx in -1..=1 {
                    if on_board(r, c + dx) {
                        let (x, y) = cell(r, c + dx);
                        surface.set_string(x, y, "▚", part_style);
                    }
                }
            }
        }
        for d in &g.debris {
            let (r, c) = d.pos;
            if on_board(r, c) {
                let (x, y) = cell(r, c);
                surface.set_string(x, y, "▩", hazard_style);
            }
        }
        for a in &g.asteroids {
            let (r, c) = a.pos;
            if on_board(r, c) {
                let (x, y) = cell(r, c);
                surface.set_string(x, y, "●", hazard_style);
            }
        }
        for m in &g.mines {
            let (r, c) = m.pos;
            if on_board(r, c) {
                let (x, y) = cell(r, c);
                surface.set_string(x, y, "◘", eshot_style);
            }
        }
        for e in &g.enemies {
            let (r, c) = e.pos;
            if on_board(r, c) {
                let (x, y) = cell(r, c);
                let style = match e.kind {
                    EnemyKind::Tank | EnemyKind::Healer => tank_style,
                    _ if e.charge > 0 => beam_style,
                    _ => enemy_style,
                };
                surface.set_string(x, y, e.kind.glyph(), style);
            }
        }
        for p in &g.powerups {
            let (r, c) = p.pos;
            if on_board(r, c) {
                let (x, y) = cell(r, c);
                surface.set_string(x, y, p.kind.glyph(), power_style);
            }
        }
        for s in &g.enemy_shots {
            let (r, c) = s.pos;
            if on_board(r, c) {
                let (x, y) = cell(r, c);
                surface.set_string(x, y, s.kind.glyph(), eshot_style);
            }
        }
        // Player shots last so they win a shared cell.
        for s in &g.shots {
            for dx in -s.half_width..=s.half_width {
                let (r, c) = (s.pos.0, s.pos.1 + dx);
                if on_board(r, c) {
                    let (x, y) = cell(r, c);
                    let style = if s.pierce { beam_style } else { shot_style };
                    surface.set_string(x, y, s.kind.glyph(), style);
                }
            }
        }
        for &side in &g.drones {
            let (r, c) = (g.ship.0, g.ship.1 + side * DRONE_OFFSET);
            if on_board(r, c) {
                let (x, y) = cell(r, c);
                surface.set_string(x, y, "◇", drone_style);
            }
        }
        // The hull itself, blinking while its invulnerability runs out, ringed
        // by the bulwark while that holds.
        if g.bulwark > 0 {
            for (dr, dc) in [(0, -1), (0, 1), (-1, 0)] {
                let (r, c) = (g.ship.0 + dr, g.ship.1 + dc);
                if on_board(r, c) {
                    let (x, y) = cell(r, c);
                    surface.set_string(x, y, "◌", beam_style);
                }
            }
        }
        if !g.invulnerable() || self.frames % 6 < 3 {
            let (x, y) = cell(g.ship.0, g.ship.1);
            surface.set_string(x, y, g.class.glyph(), ship_style);
        }

        if g.banner > 0 {
            let banner = format!(
                "◈ {} · {} — {} ◈",
                g.sector.name().to_uppercase(),
                g.node.terrain.name(),
                g.node.bonus.label()
            );
            let x = ox + (W as u16).saturating_sub(banner.chars().count() as u16) / 2;
            let (_, y) = cell(H / 2, 0);
            surface.set_string(x, y, &banner, header_style);
        }

        let status_y = top + 2 + H as u16;
        let status = match g.status {
            Status::Lost => format!(
                "Game over — score {}, wave {}, pilot level {}.  r: same build  n: new  q: quit",
                g.score, g.wave, g.level
            ),
            Status::WaveClear => format!(
                "Wave {} cleared — salvage {}.  Docking at the hangar…",
                g.wave, g.credits
            ),
            Status::Playing if self.paused => {
                "Paused — p resume · r retry · n new · q quit".to_string()
            }
            Status::Playing => {
                "←/→/↑/↓ fly · SPC fire · x special · b bomb · p pause · n new · q quit".to_string()
            }
            Status::Select | Status::Hangar => String::new(),
        };
        surface.set_string(ox, status_y, &status, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cruiser one keypress into wave one, on an empty stretch of court.
    pub(super) fn flying() -> Game {
        let mut g = Game::new(1);
        g.start(ShipClass::Cruiser, Difficulty::Normal);
        g.node.terrain = TerrainKind::Open;
        g.terrain = Terrain::new(TerrainKind::Open, 1);
        g.turrets.clear();
        g.hazards.clear();
        g.enemies.clear();
        g.mines.clear();
        g.debris.clear();
        g.asteroids.clear();
        g
    }

    /// The same, but parked in the hangar with salvage to spend.
    fn docked(credits: u32) -> Game {
        let mut g = flying();
        g.status = Status::Hangar;
        g.credits = credits;
        g
    }

    #[test]
    fn the_spread_gun_fires_one_bolt_per_lane() {
        let mut g = flying();
        g.weapon = Weapon::Spread;
        g.fire();
        let mut drifts: Vec<i16> = g.shots.iter().map(|s| s.drift).collect();
        drifts.sort_unstable();
        assert_eq!(drifts, vec![-1, 0, 1], "level one spreads three ways");
        g.weapon_level = 2;
        g.fire_cooldown = 0;
        g.shots.clear();
        g.fire();
        assert_eq!(g.shots.len(), 5, "level two widens the spread to five");
    }

    #[test]
    fn the_cadence_gates_the_trigger() {
        let mut g = flying();
        g.fire();
        let after_first = g.shots.len();
        g.fire();
        assert_eq!(
            g.shots.len(),
            after_first,
            "a second trigger pull inside the cadence is dropped"
        );
        for _ in 0..g.cadence() {
            g.tick_timers();
        }
        g.fire();
        assert!(
            g.shots.len() > after_first,
            "the cadence lets the next shot out"
        );
    }

    #[test]
    fn a_laser_beam_pierces_the_hull_it_kills() {
        let mut g = flying();
        g.weapon = Weapon::Laser;
        let col = g.ship.1;
        g.enemies = vec![
            Enemy::new(EnemyKind::Grunt, (g.ship.0 - 2, col)),
            Enemy::new(EnemyKind::Grunt, (g.ship.0 - 3, col)),
        ];
        g.fire();
        g.advance_shots();
        assert!(
            g.enemies.is_empty(),
            "one beam takes out both hulls in its lane"
        );
        assert_eq!(g.shots.len(), 1, "and keeps flying");
    }

    #[test]
    fn a_bolt_stops_at_the_hull_it_hits() {
        let mut g = flying();
        let col = g.ship.1;
        g.enemies = vec![
            Enemy::new(EnemyKind::Grunt, (g.ship.0 - 2, col)),
            Enemy::new(EnemyKind::Grunt, (g.ship.0 - 3, col)),
        ];
        g.fire();
        g.advance_shots();
        assert_eq!(g.enemies.len(), 1, "the bolt only kills the first hull");
        assert!(g.shots.is_empty(), "and is spent doing it");
    }

    #[test]
    fn homing_missiles_steer_toward_the_nearest_hull() {
        let mut g = flying();
        g.weapon = Weapon::Homing;
        g.ship.1 = 30;
        g.enemies = vec![Enemy::new(EnemyKind::Grunt, (4, 10))];
        g.fire();
        g.advance_shots();
        assert_eq!(g.shots[0].drift, -1, "the missile leans toward the target");
        assert!(g.shots[0].pos.1 < 30, "and has already closed a column");
    }

    #[test]
    fn plasma_damages_the_whole_footprint() {
        let mut g = flying();
        g.weapon = Weapon::Plasma;
        let col = g.ship.1;
        g.enemies = vec![
            Enemy::new(EnemyKind::Grunt, (g.ship.0 - 2, col - 1)),
            Enemy::new(EnemyKind::Grunt, (g.ship.0 - 2, col + 1)),
        ];
        g.fire();
        g.advance_shots();
        assert!(
            g.enemies.is_empty(),
            "the wide bolt clears the cells either side of it"
        );
    }

    #[test]
    fn wing_drones_fire_with_the_hull_until_a_surge_stuns_them() {
        let mut g = flying();
        g.drones = vec![-1, 1];
        g.fire();
        assert_eq!(g.shots.len(), 3, "the hull's bolt plus one per drone");
        g.shots.clear();
        g.fire_cooldown = 0;
        g.drone_stun = 10;
        g.fire();
        assert_eq!(g.shots.len(), 1, "stunned drones sit the volley out");
    }

    #[test]
    fn a_shield_pip_soaks_a_hit_before_a_life_is_lost() {
        let mut g = flying();
        g.enemy_shots = vec![Shot::enemy((g.ship.0 - 1, g.ship.1), 0, 1)];
        let (lives, shield) = (g.lives, g.shield);
        g.advance_enemy_shots();
        assert_eq!(g.shield, shield - 1, "the hit burns a shield pip");
        assert_eq!(g.lives, lives, "the life is untouched");
    }

    #[test]
    fn losing_the_last_pip_costs_a_life_and_downgrades_the_gun() {
        let mut g = flying();
        g.shield = 0;
        g.weapon_level = 3;
        g.drones = vec![-1];
        g.enemy_shots = vec![Shot::enemy((g.ship.0 - 1, g.ship.1), 0, 1)];
        let lives = g.lives;
        g.advance_enemy_shots();
        assert_eq!(g.lives, lives - 1, "the life goes");
        assert_eq!(g.shield, g.max_shield, "shields come back for the next one");
        assert_eq!(g.weapon_level, 2, "the gun drops a level");
        assert!(g.drones.is_empty(), "and a drone is shaken loose");
    }

    #[test]
    fn invulnerability_swallows_a_second_hit_in_the_same_moment() {
        let mut g = flying();
        let row = g.ship.0 - 1;
        g.enemy_shots = vec![
            Shot::enemy((row, g.ship.1), 0, 1),
            Shot::enemy((row, g.ship.1), 0, 1),
        ];
        let shield = g.shield;
        g.advance_enemy_shots();
        assert_eq!(
            g.shield,
            shield - 1,
            "two shots landing together only cost one pip"
        );
    }

    #[test]
    fn the_bulwark_eats_fire_outright() {
        let mut g = flying();
        g.bulwark = 10;
        g.enemy_shots = vec![Shot::enemy((g.ship.0 - 1, g.ship.1), 0, 1)];
        let shield = g.shield;
        g.advance_enemy_shots();
        assert_eq!(g.shield, shield, "nothing gets through the bubble");
        assert!(g.enemy_shots.is_empty(), "and the shot is gone");
    }

    #[test]
    fn a_blink_jumps_the_hull_and_costs_energy() {
        let mut g = Game::new(1);
        g.start(ShipClass::Interceptor, Difficulty::Normal);
        let (col, energy) = (g.ship.1, g.energy);
        g.move_ship(1, 0);
        g.special();
        assert_eq!(
            g.ship.1,
            (col + g.thrust() + BLINK_DISTANCE).min(W - 2),
            "the hull jumps the way it was flying"
        );
        assert_eq!(g.energy, energy - g.special_cost(), "and the meter pays");
        assert!(g.invulnerable(), "landing invulnerable");
    }

    #[test]
    fn a_barrage_lays_bolts_across_the_whole_court() {
        let mut g = Game::new(1);
        g.start(ShipClass::Juggernaut, Difficulty::Normal);
        g.special();
        assert!(g.shots.len() >= 8, "the wall covers the court");
        let spread = g.shots.last().unwrap().pos.1 - g.shots[0].pos.1;
        assert!(spread > W / 2, "from one wall to the other");
    }

    #[test]
    fn overdrive_makes_the_special_cheaper() {
        let mut g = flying();
        let full = g.special_cost();
        g.loadout.modules.push(Module::Overdrive);
        assert!(g.special_cost() < full, "the module discounts the special");
    }

    #[test]
    fn a_matching_gun_pickup_levels_it_up_and_a_different_one_swaps_it() {
        let mut g = flying();
        g.collect(PowerKind::Gun(Weapon::Blaster));
        assert_eq!(g.weapon_level, 2, "the carried gun levels up");
        g.collect(PowerKind::Gun(Weapon::Laser));
        assert_eq!(g.weapon, Weapon::Laser, "a different gun replaces it");
        for _ in 0..5 {
            g.collect(PowerKind::Gun(Weapon::Laser));
        }
        assert_eq!(g.weapon_level, MAX_WEAPON_LEVEL, "levels are capped");
    }

    #[test]
    fn a_pickup_is_collected_by_flying_into_it() {
        let mut g = flying();
        g.bombs = 0;
        g.powerups = vec![Powerup {
            pos: (g.ship.0 - 1, g.ship.1),
            kind: PowerKind::Bomb,
        }];
        g.advance_powerups();
        assert_eq!(g.bombs, 1, "the bomb is picked up");
        assert!(g.powerups.is_empty(), "and leaves the court");
    }

    #[test]
    fn the_magnet_pulls_pickups_toward_the_hull() {
        let mut g = flying();
        g.loadout.modules.push(Module::Magnet);
        g.powerups = vec![Powerup {
            pos: (g.ship.0 - 4, g.ship.1 + 6),
            kind: PowerKind::Medal,
        }];
        g.advance_powerups();
        assert_eq!(
            g.powerups[0].pos.1,
            g.ship.1 + 5,
            "the medal slides a column closer"
        );
    }

    #[test]
    fn the_repair_bay_hands_a_pip_back_on_its_cadence() {
        let mut g = flying();
        g.loadout.modules.push(Module::RepairBay);
        g.shield = 0;
        g.repair_timer = 1;
        g.tick_timers();
        assert_eq!(g.shield, 1, "the bay patches a pip in");
        assert_eq!(g.repair_timer, REPAIR_CADENCE, "and resets its clock");
    }

    #[test]
    fn a_smart_bomb_clears_enemy_fire_and_the_lighter_hulls() {
        let mut g = flying();
        g.enemies = vec![
            Enemy::new(EnemyKind::Grunt, (5, 10)),
            Enemy::new(EnemyKind::Tank, (5, 20)),
        ];
        g.enemy_shots = vec![Shot::enemy((10, 10), 0, 1)];
        let bombs = g.bombs;
        g.bomb();
        assert_eq!(g.bombs, bombs - 1, "the bomb is spent");
        assert!(g.enemy_shots.is_empty(), "enemy fire is wiped");
        assert_eq!(g.enemies.len(), 1, "the grunt dies");
        assert_eq!(
            g.enemies[0].hp,
            EnemyKind::Tank.hp() - BOMB_DAMAGE,
            "the tank survives with its armour scarred"
        );
    }

    #[test]
    fn the_kill_chain_multiplies_the_score() {
        let mut g = flying();
        g.score = 0;
        g.award(10);
        g.award(10);
        g.award(10);
        assert_eq!(g.score, 10 + 20 + 30, "each kill in the chain pays more");
        g.damage_ship(1);
        assert_eq!(g.combo, 1, "taking a hit breaks the chain");
    }

    #[test]
    fn the_chain_cools_off_when_the_killing_stops() {
        let mut g = flying();
        g.award(10);
        assert_eq!(g.combo, 2);
        for _ in 0..COMBO_TICKS {
            g.tick_timers();
        }
        assert_eq!(g.combo, 1, "the chain lapses after COMBO_TICKS quiet ticks");
    }

    #[test]
    fn kills_pay_salvage_and_the_salvager_pays_more() {
        let mut g = flying();
        g.credits = 0;
        g.award(100);
        let plain = g.credits;
        assert!(plain > 0, "a kill banks salvage");
        g.credits = 0;
        g.combo = 1;
        g.loadout.modules.push(Module::Salvager);
        g.award(100);
        assert!(g.credits > plain, "the salvager takes a bigger cut");
    }

    #[test]
    fn experience_levels_the_pilot_and_pays_an_upgrade() {
        let mut g = flying();
        let damage = g.gun_damage();
        g.gain_xp(4 * XP_PER_LEVEL);
        assert_eq!(g.level, 2, "the bar fills and the pilot levels");
        assert_eq!(
            LevelReward::of_level(2),
            LevelReward::Firepower,
            "level two is the firepower slot"
        );
        assert_eq!(g.gun_damage(), damage + 1, "and the guns hit harder");
        assert_eq!(g.xp_next, XP_PER_LEVEL * 2, "the next level costs more");
    }

    #[test]
    fn an_extend_is_paid_at_every_threshold() {
        let mut g = flying();
        let lives = g.lives;
        g.add_score(EXTEND_SCORE);
        assert_eq!(g.lives, lives + 1, "crossing the threshold pays a hull");
        assert_eq!(g.next_extend, EXTEND_SCORE * 2, "and the bar moves up");
    }

    #[test]
    fn a_sniper_telegraphs_before_its_shot_goes_off() {
        let mut g = flying();
        let mut sniper = g.hatch(EnemyKind::Sniper, (4, g.ship.1));
        sniper.charge = 1;
        g.enemies = vec![sniper];
        g.advance_enemies();
        assert_eq!(g.enemies[0].charge, 0, "the telegraph runs out");
        assert_eq!(g.enemy_shots.len(), 1, "and the shot goes off");
        assert_eq!(g.enemy_shots[0].speed, 2, "twice as fast as anything else");
    }

    #[test]
    fn a_mine_goes_off_when_the_ship_flies_close() {
        let mut g = flying();
        g.mines = vec![Mine {
            pos: (g.ship.0 - 1, g.ship.1),
            fuse: MINE_FUSE,
        }];
        g.advance_mines();
        assert!(g.mines.is_empty(), "the mine is spent");
        assert_eq!(g.enemy_shots.len(), 3, "throwing a three-way spread");
    }

    #[test]
    fn shooting_a_mine_sets_it_off_too() {
        let mut g = flying();
        let pos = (6, 20);
        g.mines = vec![Mine {
            pos,
            fuse: MINE_FUSE,
        }];
        let hit = g.hit_targets(&Shot::bolt(pos, 0, 2));
        assert!(hit, "the shot connects with the mine");
        assert!(g.mines.is_empty());
        assert_eq!(g.enemy_shots.len(), 3, "and the mine still blows");
    }

    #[test]
    fn a_splitter_breaks_into_two_divers() {
        let mut g = flying();
        let pos = (6, 20);
        let mut splitter = Enemy::new(EnemyKind::Splitter, pos);
        splitter.hp = 1;
        g.enemies = vec![splitter];
        g.hit_targets(&Shot::bolt(pos, 0, 2));
        assert_eq!(g.enemies.len(), 2, "two grunts come out of the wreck");
        assert!(
            g.enemies
                .iter()
                .all(|e| matches!(e.state, EnemyState::Diving { .. })),
            "both dive straight at the ship"
        );
    }

    #[test]
    fn a_healer_patches_up_the_hull_beside_it() {
        let mut g = flying();
        let mut hurt = g.hatch(EnemyKind::Tank, (5, 20));
        hurt.hp = 2;
        g.enemies = vec![g.hatch(EnemyKind::Healer, (5, 22)), hurt];
        g.tick = HEAL_CADENCE;
        g.advance_enemies();
        let tank = g
            .enemies
            .iter()
            .find(|e| e.kind == EnemyKind::Tank)
            .expect("the tank is still flying");
        assert_eq!(tank.hp, 3, "the healer welds a hit point back on");
    }

    #[test]
    fn a_diver_that_misses_returns_to_formation_but_a_kamikaze_is_gone() {
        let mut g = flying();
        let mut grunt = Enemy::new(EnemyKind::Grunt, (4, 8));
        grunt.pos = (H - 1, 8);
        grunt.state = EnemyState::Diving { target_x: 8 };
        let mut kamikaze = Enemy::new(EnemyKind::Kamikaze, (4, 40));
        kamikaze.pos = (H - 1, 40);
        kamikaze.state = EnemyState::Diving { target_x: 40 };
        g.enemies = vec![grunt, kamikaze];
        g.advance_enemies();
        assert_eq!(g.enemies.len(), 1, "the kamikaze leaves the court");
        assert_eq!(g.enemies[0].kind, EnemyKind::Grunt);
        assert_eq!(
            g.enemies[0].state,
            EnemyState::Formation,
            "the diver re-forms"
        );
        assert_eq!(g.enemies[0].pos, (4, 8), "back in its slot");
    }

    #[test]
    fn ramming_the_ship_costs_the_hull_a_pip() {
        let mut g = flying();
        let mut kamikaze = Enemy::new(EnemyKind::Kamikaze, (4, g.ship.1));
        kamikaze.pos = (g.ship.0 - 2, g.ship.1);
        kamikaze.state = EnemyState::Diving { target_x: g.ship.1 };
        g.enemies = vec![kamikaze];
        let shield = g.shield;
        g.advance_enemies();
        assert_eq!(g.shield, shield - 1, "the ram lands");
        assert!(g.enemies.is_empty(), "and the kamikaze is spent");
    }

    #[test]
    fn a_rock_can_be_shot_or_will_ram_the_hull() {
        let mut g = flying();
        let pos = (6, 20);
        g.asteroids = vec![Asteroid {
            pos,
            hp: 2,
            drift: 0,
        }];
        g.hit_targets(&Shot::bolt(pos, 0, 3));
        assert!(g.asteroids.is_empty(), "a big enough shot breaks the rock");
        g.asteroids = vec![Asteroid {
            pos: g.ship,
            hp: 3,
            drift: 0,
        }];
        g.invuln = 0;
        g.tick = 1;
        let shield = g.shield;
        g.advance_asteroids();
        assert_eq!(g.shield, shield - 1, "flying into one costs a pip");
    }

    #[test]
    fn a_hulk_eats_shots_from_both_sides() {
        let mut g = flying();
        let pos = (6, 20);
        g.debris = vec![Debris { pos, hp: 4 }];
        assert!(g.hit_targets(&Shot::bolt(pos, 0, 1)), "player fire lands");
        assert_eq!(g.debris[0].hp, 3, "and chips the hulk");
        g.enemy_shots = vec![Shot::enemy((pos.0 - 1, pos.1), 0, 1)];
        g.advance_enemy_shots();
        assert!(g.enemy_shots.is_empty(), "enemy fire is stopped too");
        assert_eq!(g.debris[0].hp, 2, "chipping it further");
    }

    #[test]
    fn sectors_change_what_is_in_the_court() {
        let mut g = flying();
        g.sector = Sector::Minefield;
        g.dress_sector();
        assert_eq!(
            g.mines.len(),
            Sector::Minefield.starting_mines(),
            "a minefield starts sown"
        );
        let mut g = flying();
        g.sector = Sector::DebrisRing;
        g.dress_sector();
        assert_eq!(
            g.debris.len(),
            Sector::DebrisRing.debris_blocks(),
            "a debris ring starts blocked"
        );
        assert!(
            Sector::AsteroidBelt.asteroid_chance() < Sector::OpenSpace.asteroid_chance(),
            "the belt throws far more rock"
        );
        let sectors: Vec<Sector> = (1..=6).map(Sector::of_wave).collect();
        assert_eq!(
            sectors.len(),
            sectors
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            "six waves visit six different sectors"
        );
    }

    #[test]
    fn the_nebula_drags_every_shot_a_row_slower() {
        let mut g = flying();
        g.fire();
        let clear = g.shots[0].speed;
        let mut g = flying();
        g.sector = Sector::Nebula;
        g.fire();
        assert_eq!(g.shots[0].speed, clear + 1, "the soup costs a row of speed");
        g.launch_enemy(Shot::enemy((0, 5), 0, 2));
        assert_eq!(g.enemy_shots[0].speed, 1, "enemy fire wades too");
    }

    #[test]
    fn an_ion_surge_drains_the_reactor_and_stuns_the_drones() {
        let mut g = flying();
        g.sector = Sector::IonStorm;
        g.drones = vec![-1];
        g.tick = Sector::IonStorm.surge_cadence();
        let energy = g.energy;
        g.advance_storm();
        assert!(g.energy < energy, "the surge drains the meter");
        assert_eq!(g.drone_stun, SURGE_STUN, "and knocks the drones out");
        assert_eq!(g.enemy_shots.len(), 3, "lightning comes down three lanes");
    }

    #[test]
    fn formations_change_shape_from_wave_to_wave() {
        let grid = Formation::Grid.slot(0, 0);
        let vee = Formation::Vee.slot(0, 0);
        assert_ne!(grid, vee, "the vee droops at the edges");
        assert_eq!(
            Formation::Grid.slot(0, 4).1,
            Formation::Vee.slot(0, 4).1,
            "but the columns line up either way"
        );
        let shapes: Vec<Formation> = (1..=5).map(Formation::of_wave).collect();
        assert_eq!(
            shapes.len(),
            shapes
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            "five waves fly five shapes"
        );
    }

    #[test]
    fn every_fourth_wave_is_a_boss_wave_and_the_bosses_cycle() {
        let mut g = flying();
        g.wave = 3;
        g.spawn_wave();
        assert!(g.boss.is_none(), "wave three is a formation");
        g.wave = BOSS_EVERY;
        g.spawn_wave();
        assert!(g.boss.is_some(), "wave four brings the boss");
        assert!(!g.enemies.is_empty(), "with a kamikaze escort alongside it");
        let bosses: Vec<BossKind> = (1..=4).map(|n| BossKind::of_wave(n * BOSS_EVERY)).collect();
        assert_eq!(
            bosses.len(),
            bosses
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            "four boss waves fight four different bosses"
        );
    }

    #[test]
    fn the_boss_enrages_as_its_armour_burns_off() {
        let mut boss = Boss::new(BossKind::Dreadnought, 100);
        assert_eq!(boss.phase(), 1);
        boss.hp = 50;
        assert_eq!(boss.phase(), 2, "past a third of the hull it escalates");
        boss.hp = 20;
        assert_eq!(boss.phase(), 3, "and enrages below a third");
        assert_eq!(boss.speed(), 2, "the enraged sweep is twice as fast");
        assert!(boss.cadence() < 14, "and it fires far more often");
    }

    #[test]
    fn the_twin_core_is_armoured_until_both_turrets_fall() {
        let mut g = flying();
        g.boss = Some(Boss::new(BossKind::Twin, 90));
        let core = g.boss.as_ref().unwrap().pos;
        let before = g.boss.as_ref().unwrap().hp;
        g.hit_targets(&Shot::bolt(core, 0, 5));
        assert_eq!(
            g.boss.as_ref().unwrap().hp,
            before,
            "the core shrugs the shot off while the turrets stand"
        );
        g.boss.as_mut().unwrap().parts.clear();
        g.hit_targets(&Shot::bolt(core, 0, 5));
        assert_eq!(
            g.boss.as_ref().unwrap().hp,
            before - 5,
            "with the turrets gone the core takes it"
        );
    }

    #[test]
    fn a_turret_can_be_shot_off_the_twin() {
        let mut g = flying();
        g.boss = Some(Boss::new(BossKind::Twin, 90));
        let turret = g.boss.as_ref().unwrap().part_cells()[0];
        let hp = g.boss.as_ref().unwrap().parts[0].hp;
        g.hit_targets(&Shot::bolt(turret, 0, hp));
        assert_eq!(
            g.boss.as_ref().unwrap().parts.len(),
            1,
            "the turret comes off"
        );
    }

    #[test]
    fn the_serpent_body_swims_behind_its_head() {
        let mut boss = Boss::new(BossKind::Serpent, 120);
        assert_eq!(boss.parts.len(), SERPENT_SEGMENTS);
        let first = boss.part_cells();
        boss.tick += 2;
        let later = boss.part_cells();
        assert_ne!(first, later, "the segments weave as the tick advances");
        assert!(
            later.iter().all(|&(_, c)| c < boss.pos.1),
            "and always trail the head"
        );
    }

    #[test]
    fn killing_the_boss_pays_a_bounty_and_clears_the_wave() {
        let mut g = flying();
        g.wave = BOSS_EVERY;
        g.spawn_wave();
        g.enemies.clear();
        let score = g.score;
        if let Some(boss) = g.boss.as_mut() {
            boss.hp = 0;
            boss.parts.clear();
        }
        g.check_end();
        assert!(g.boss.is_none(), "the boss is destroyed");
        assert_eq!(g.status, Status::WaveClear);
        assert!(
            g.score >= score + 500 * BOSS_EVERY,
            "the bounty and the wave bonus are paid"
        );
    }

    #[test]
    fn a_cleared_wave_docks_at_the_hangar() {
        let mut g = flying();
        g.check_end();
        assert_eq!(g.status, Status::WaveClear, "an empty court ends the wave");
        for _ in 0..INTERMISSION_TICKS {
            g.step();
        }
        assert_eq!(g.status, Status::Hangar, "and the hangar opens");
        assert!(g.credits > 0, "with the wave bonus banked as salvage");
    }

    #[test]
    fn the_hangar_sells_component_tiers() {
        let mut g = docked(5_000);
        let line = g
            .shop_lines()
            .into_iter()
            .find(|l| l.entry == ShopEntry::Component(Part::Engine))
            .expect("the engine is on the list");
        let thrust = g.thrust();
        assert!(g.buy(line.key), "the sale goes through");
        assert_eq!(g.loadout.tier(Part::Engine), 1, "the engine is upgraded");
        assert_eq!(g.credits, 5_000 - line.price, "and the salvage is spent");
        assert!(g.buy(line.key), "a second tier is on sale too");
        assert_eq!(g.thrust(), thrust + 1, "two tiers buy a column of thrust");
    }

    #[test]
    fn the_hangar_refuses_what_cannot_be_paid_for() {
        let mut g = docked(0);
        let line = g.shop_lines().into_iter().next().expect("stock is listed");
        assert!(!line.available, "an empty hold cannot afford it");
        assert!(!g.buy(line.key), "and the sale is refused");
        assert_eq!(g.loadout.tier(Part::Engine), 0);
    }

    #[test]
    fn hangar_plating_raises_the_shield_ceiling() {
        let mut g = docked(5_000);
        let ceiling = g.max_shield;
        let line = g
            .shop_lines()
            .into_iter()
            .find(|l| l.entry == ShopEntry::Component(Part::Plating))
            .expect("plating is on the list");
        assert!(g.buy(line.key));
        assert_eq!(g.max_shield, ceiling + 1, "the hull carries another pip");
        assert_eq!(g.shield, g.max_shield, "and it is fitted full");
    }

    #[test]
    fn hangar_fittings_are_sold_once_each() {
        let mut g = docked(5_000);
        let line = g
            .shop_lines()
            .into_iter()
            .find(|l| l.entry == ShopEntry::Fitting(Module::Magnet))
            .expect("the magnet is on the list");
        assert!(g.buy(line.key));
        assert!(g.loadout.has(Module::Magnet), "the module is fitted");
        assert!(
            !g.shop_lines()
                .iter()
                .any(|l| l.entry == ShopEntry::Fitting(Module::Magnet)),
            "and it drops off the list"
        );
    }

    #[test]
    fn hangar_consumables_respect_their_ceilings() {
        let mut g = docked(5_000);
        g.drones = vec![-1, 1];
        assert!(
            g.shop_lines()
                .iter()
                .any(|l| l.entry == ShopEntry::Consumable(Stock::Drone) && !l.available),
            "a full wing cannot take another drone"
        );
        let swap = g
            .shop_lines()
            .into_iter()
            .find(|l| l.entry == ShopEntry::Consumable(Stock::GunSwap))
            .expect("the swap is on the list");
        let next = g.weapon.next();
        assert!(g.buy(swap.key));
        assert_eq!(g.weapon, next, "the gun rotates on");
    }

    #[test]
    fn leaving_the_hangar_flies_the_next_wave_with_the_build_intact() {
        let mut g = docked(5_000);
        g.score = 4_200;
        let engine = g
            .shop_lines()
            .into_iter()
            .find(|l| l.entry == ShopEntry::Component(Part::Engine))
            .expect("the engine is on the list");
        assert!(g.buy(engine.key));
        g.launch_next_wave();
        assert_eq!(g.status, Status::Playing, "the next wave starts");
        assert_eq!(g.wave, 2);
        assert!(!g.enemies.is_empty(), "with a fresh formation");
        assert_eq!(g.loadout.tier(Part::Engine), 1, "the build carries over");
        assert_eq!(g.shield, g.max_shield, "shields are topped back up");
        assert_eq!(g.score, 4_200, "and the score is untouched");
    }

    #[test]
    fn the_run_ends_when_the_last_life_is_gone() {
        let mut g = flying();
        g.lives = 1;
        g.shield = 0;
        g.enemy_shots = vec![Shot::enemy((g.ship.0 - 1, g.ship.1), 0, 1)];
        g.advance_enemy_shots();
        g.check_end();
        assert_eq!(g.status, Status::Lost);
    }

    #[test]
    fn the_hull_cannot_fly_out_of_its_box() {
        let mut g = flying();
        for _ in 0..40 {
            g.move_ship(-1, -1);
        }
        assert_eq!(g.ship.0, SHIP_TOP, "it stops at the top of its band");
        assert_eq!(g.ship.1, 1, "and inside the left wall");
        for _ in 0..80 {
            g.move_ship(1, 1);
        }
        assert_eq!(g.ship.0, SHIP_ROW);
        assert_eq!(g.ship.1, W - 2);
    }

    #[test]
    fn a_long_run_through_every_sector_never_panics() {
        let mut g = Game::new(7);
        g.start(ShipClass::Interceptor, Difficulty::Insane);
        for i in 0..2_000 {
            if g.status == Status::Hangar {
                g.launch_next_wave();
            }
            if i % 3 == 0 {
                g.fire();
            }
            if i % 97 == 0 {
                g.bomb();
            }
            if i % 61 == 0 {
                g.special();
            }
            g.move_ship(if i % 2 == 0 { 1 } else { -1 }, 0);
            g.step();
            assert!(
                (1..W - 1).contains(&g.ship.1),
                "the hull stays on the court"
            );
            assert!(
                g.enemies.iter().all(|e| e.pos.0 < H),
                "no hull is left below the floor"
            );
            assert!(g.energy <= g.max_energy(), "the meter never overfills");
        }
    }
}

#[cfg(test)]
mod map_tests {
    use super::tests::flying;
    use super::*;

    /// A court flown through a given kind of rock.
    fn on_map(kind: TerrainKind) -> Game {
        let mut g = Game::new(3);
        g.start(ShipClass::Cruiser, Difficulty::Normal);
        g.node.terrain = kind;
        g.terrain = Terrain::new(kind, 11);
        g.enemies.clear();
        g.turrets.clear();
        g.hazards.clear();
        g.enemy_shots.clear();
        g
    }

    /// A row where the rock actually closes in, and the wall cell beside it.
    fn wall_cell(g: &Game, row: i16) -> i16 {
        let (left, right) = g.terrain.channel(row);
        if left > 1 {
            left - 1
        } else {
            right + 1
        }
    }

    #[test]
    fn terrain_kinds_shape_the_channel() {
        let tunnel = Terrain::new(TerrainKind::Tunnel, 5);
        let canyon = Terrain::new(TerrainKind::Canyon, 5);
        let open = Terrain::new(TerrainKind::Open, 5);
        let span = |t: &Terrain| {
            let (l, r) = t.channel(6);
            r - l
        };
        assert!(
            span(&tunnel) < span(&canyon),
            "a tunnel is the tightest run"
        );
        assert!(span(&canyon) < span(&open), "and open space the widest");
        assert!(
            !open.solid(6, 1),
            "open space has no rock to fly into at all"
        );
    }

    #[test]
    fn the_rock_scrolls_down_the_court() {
        let mut terrain = Terrain::new(TerrainKind::Cave, 9);
        let second = terrain.rows[1].open;
        terrain.scroll();
        assert_eq!(
            terrain.rows[2].open, second,
            "every row slides one place down"
        );
        assert_eq!(terrain.rows.len(), H as usize, "and the court stays full");
    }

    #[test]
    fn rock_stops_a_shot_and_a_cave_lets_it_be_carved() {
        let mut g = on_map(TerrainKind::Cave);
        let row = g.ship.0 - 2;
        let col = wall_cell(&g, row);
        assert!(g.terrain.solid(row, col), "the cell is rock to begin with");
        g.ship.1 = col;
        g.fire();
        g.advance_shots();
        assert!(g.shots.is_empty(), "the shot is stopped by the rock");
        assert!(!g.terrain.solid(row, col), "and a cave wall is carved open");
    }

    #[test]
    fn canyon_walls_cannot_be_carved() {
        let mut g = on_map(TerrainKind::Canyon);
        let row = g.ship.0 - 2;
        let col = wall_cell(&g, row);
        g.ship.1 = col;
        g.fire();
        g.advance_shots();
        assert!(
            g.terrain.solid(row, col),
            "canyon rock takes the hit and holds"
        );
    }

    #[test]
    fn enemy_fire_is_stopped_by_the_rock_as_well() {
        let mut g = on_map(TerrainKind::Tunnel);
        let row = g.ship.0 - 3;
        let col = wall_cell(&g, row);
        g.enemy_shots = vec![Shot::enemy((row - 1, col), 0, 1)];
        g.advance_enemy_shots();
        assert!(g.enemy_shots.is_empty(), "the wall eats it");
    }

    #[test]
    fn rock_closing_in_shoves_the_hull_back_into_the_lane() {
        let mut g = on_map(TerrainKind::Tunnel);
        g.tick = 1;
        let col = wall_cell(&g, g.ship.0);
        g.ship.1 = col;
        let shield = g.shield;
        g.advance_terrain();
        assert_eq!(g.shield, shield, "being shoved aside does not cost a pip");
        let (left, right) = g.terrain.channel(g.ship.0);
        assert!(
            (left..=right).contains(&g.ship.1),
            "and is shoved back into the lane"
        );
    }

    #[test]
    fn a_wall_turret_fires_and_can_be_shot_off() {
        let mut g = on_map(TerrainKind::Canyon);
        g.tick = 1;
        g.turrets = vec![WallTurret {
            pos: (5, 12),
            hp: 3,
            cooldown: 0,
        }];
        g.advance_terrain();
        assert_eq!(g.enemy_shots.len(), 1, "the emplacement opens up");
        assert!(g.hit_targets(&Shot::bolt((5, 12), 0, 3)), "and can be hit");
        assert!(g.turrets.is_empty(), "three points of damage takes it out");
    }

    #[test]
    fn a_gravity_well_drags_the_hull_toward_it() {
        let mut g = on_map(TerrainKind::Open);
        g.hazards = vec![Hazard::GravityWell {
            pos: (4, g.ship.1 + 6),
        }];
        g.tick = 2;
        let col = g.ship.1;
        g.advance_hazards();
        assert_eq!(g.ship.1, col + 1, "the well pulls a column a time");
    }

    #[test]
    fn an_ion_stream_shoves_the_hull_sideways() {
        let mut g = on_map(TerrainKind::Open);
        g.hazards = vec![Hazard::IonStream { push: -1 }];
        g.tick = 3;
        let col = g.ship.1;
        g.advance_hazards();
        assert_eq!(g.ship.1, col - 1, "the current carries the hull with it");
    }

    #[test]
    fn a_solar_flare_burns_whatever_is_in_its_column() {
        let mut g = on_map(TerrainKind::Open);
        g.shield = 4;
        g.hazards = vec![Hazard::SolarFlare {
            col: g.ship.1,
            dir: 1,
        }];
        g.tick = 1;
        g.advance_hazards();
        assert_eq!(g.shield, 3, "standing in a hot flare costs a pip");
        g.invuln = 0;
        g.tick = FLARE_ACTIVE + 1;
        g.advance_hazards();
        assert_eq!(g.shield, 3, "but the column is cold between pulses");
        g.tick = FLARE_CADENCE;
        g.invuln = 0;
        g.advance_hazards();
        match g.hazards[0] {
            Hazard::SolarFlare { col, .. } => {
                assert_ne!(col, g.ship.1, "and the wall of fire sweeps on")
            }
            _ => panic!("the flare is still the hazard"),
        }
    }

    #[test]
    fn the_chart_offers_three_stops_and_flies_the_one_you_pick() {
        let mut g = flying();
        g.check_end();
        for _ in 0..INTERMISSION_TICKS {
            g.step();
        }
        assert_eq!(g.status, Status::Hangar);
        assert_eq!(g.route.len(), ROUTE_CHOICES, "the chart offers three stops");
        assert!(g.choose_route(2), "the third is a legal pick");
        let picked = g.node;
        assert!(!g.choose_route(9), "and nothing beyond the chart is");
        g.launch_next_wave();
        assert_eq!(g.sector, picked.sector, "the run flies the stop you picked");
        assert_eq!(g.node.terrain, picked.terrain, "through its rock");
    }

    #[test]
    fn a_salvage_cache_pays_out_on_arrival() {
        let mut g = flying();
        g.credits = 0;
        g.node = RouteNode {
            sector: Sector::OpenSpace,
            terrain: TerrainKind::Open,
            bonus: NodeBonus::Cache(750),
        };
        g.spawn_wave();
        assert_eq!(g.credits, 750, "the cache is banked when the wave starts");
    }

    #[test]
    fn an_armoury_stop_hands_over_its_gun() {
        let mut g = flying();
        g.weapon = Weapon::Blaster;
        g.weapon_level = 1;
        g.node = RouteNode {
            sector: Sector::OpenSpace,
            terrain: TerrainKind::Open,
            bonus: NodeBonus::Armoury(Weapon::Plasma),
        };
        g.spawn_wave();
        assert_eq!(g.weapon, Weapon::Plasma, "the crate is fitted");
        assert!(g.weapon_level >= 2, "and comes half tuned");
    }

    #[test]
    fn a_danger_run_is_armoured_but_pays_double() {
        let mut g = flying();
        let plain = g.wave_armour();
        g.credits = 0;
        g.award(100);
        let plain_salvage = g.credits;
        g.node.bonus = NodeBonus::Danger;
        assert_eq!(
            g.wave_armour(),
            plain + 2,
            "everything out there is tougher"
        );
        g.credits = 0;
        g.combo = 1;
        g.award(100);
        assert_eq!(g.credits, plain_salvage * 2, "and worth twice as much");
    }

    #[test]
    fn the_formation_is_squeezed_into_the_channel() {
        let mut g = flying();
        g.wave = 2;
        g.node = RouteNode {
            sector: Sector::OpenSpace,
            terrain: TerrainKind::Tunnel,
            bonus: NodeBonus::Refit,
        };
        g.spawn_wave();
        assert!(!g.enemies.is_empty());
        for e in &g.enemies {
            let (left, right) = g.terrain.channel(e.home.0);
            assert!(
                (left..=right).contains(&e.home.1),
                "every hull in the wave holds station inside the rock"
            );
        }
    }

    #[test]
    fn a_long_run_through_the_rock_never_panics() {
        let mut g = Game::new(21);
        g.start(ShipClass::Juggernaut, Difficulty::Hard);
        for i in 0..3_000 {
            if g.status == Status::Hangar {
                g.choose_route(i % ROUTE_CHOICES);
                g.launch_next_wave();
            }
            if i % 3 == 0 {
                g.fire();
            }
            g.move_ship(if i % 5 < 2 { 1 } else { -1 }, 0);
            g.step();
            assert!(
                (1..W - 1).contains(&g.ship.1),
                "the hull stays on the court"
            );
            assert_eq!(
                g.terrain.rows.len(),
                H as usize,
                "the rock always fills the court"
            );
        }
    }
}

#[cfg(test)]
mod gun_tests {
    use super::tests::flying;
    use super::*;

    #[test]
    fn the_vulcan_hoses_faster_than_anything_else() {
        let mut g = flying();
        g.weapon = Weapon::Blaster;
        let blaster = g.cadence();
        g.weapon = Weapon::Vulcan;
        assert!(g.cadence() < blaster, "the machine gun barely pauses");
        g.weapon = Weapon::Rail;
        assert!(g.cadence() > blaster, "the rail gun takes its time");
        g.weapon = Weapon::Vulcan;
        g.weapon_level = 3;
        g.fire();
        assert_eq!(g.shots.len(), 3, "and walks three rounds across at level 3");
    }

    #[test]
    fn a_rocket_takes_the_neighbours_with_it() {
        let mut g = flying();
        g.weapon = Weapon::Rocket;
        let (row, col) = (g.ship.0 - 2, g.ship.1);
        g.enemies = vec![
            Enemy::new(EnemyKind::Grunt, (row, col)),
            Enemy::new(EnemyKind::Grunt, (row, col + 1)),
            Enemy::new(EnemyKind::Grunt, (row - 1, col - 1)),
        ];
        g.fire();
        g.advance_shots();
        assert!(g.enemies.is_empty(), "the blast clears the cells around it");
    }

    #[test]
    fn a_flak_shell_bursts_into_a_fan() {
        let mut g = flying();
        g.weapon = Weapon::Flak;
        g.fire();
        assert_eq!(g.shots.len(), 1, "one shell goes up");
        for _ in 0..FLAK_FUSE {
            g.advance_shots();
        }
        assert_eq!(g.shots.len(), 5, "and comes apart into five fragments");
        assert!(
            g.shots.iter().all(|s| s.fuse == 0),
            "fragments have no fuse of their own"
        );
    }

    #[test]
    fn a_rail_slug_runs_the_length_of_the_court() {
        let mut g = flying();
        g.weapon = Weapon::Rail;
        let col = g.ship.1;
        g.enemies = (2..=5)
            .map(|dr| Enemy::new(EnemyKind::Tank, (g.ship.0 - dr, col)))
            .collect();
        g.fire();
        g.advance_shots();
        assert!(
            g.enemies.is_empty(),
            "the slug punches through the whole column"
        );
    }

    #[test]
    fn an_arc_bolt_earths_itself_through_a_crowd() {
        let mut g = flying();
        g.weapon = Weapon::Arc;
        g.weapon_level = 2;
        let (row, col) = (g.ship.0 - 2, g.ship.1);
        g.enemies = vec![
            Enemy::new(EnemyKind::Grunt, (row, col)),
            Enemy::new(EnemyKind::Grunt, (row, col + 3)),
            Enemy::new(EnemyKind::Grunt, (row - 1, col + 5)),
        ];
        g.fire();
        g.advance_shots();
        assert!(
            g.enemies.len() <= 1,
            "the bolt jumps from hull to hull, {} left",
            g.enemies.len()
        );
    }

    #[test]
    fn every_gun_puts_something_in_the_air() {
        for weapon in Weapon::ALL {
            let mut g = flying();
            g.weapon = weapon;
            g.weapon_level = 3;
            g.fire();
            assert!(
                !g.shots.is_empty(),
                "{} fires something at level three",
                weapon.name()
            );
        }
    }
}

#[cfg(test)]
mod terrain_kind_tests {
    use super::tests::flying;
    use super::*;

    fn count_pillars(kind: TerrainKind) -> usize {
        Terrain::new(kind, 17)
            .rows
            .iter()
            .map(|r| r.pillars.len())
            .sum()
    }

    #[test]
    fn gates_leave_exactly_one_gap_to_thread() {
        let terrain = Terrain::new(TerrainKind::Gates, 4);
        let gate = terrain
            .rows
            .iter()
            .find(|r| r.open.1 - r.open.0 <= GATE_GAP)
            .expect("a bulkhead stands somewhere in the court");
        assert!(gate.open.0 > 1, "rock to the left of the gap");
        assert!(gate.open.1 < W - 2, "and rock to the right of it");
        assert!(
            terrain.rows.iter().any(|r| r.open == (1, W - 2)),
            "with clear rows between the bulkheads"
        );
    }

    #[test]
    fn a_spine_splits_the_court_down_the_middle() {
        let terrain = Terrain::new(TerrainKind::Spine, 4);
        assert!(
            terrain.rows.iter().all(|r| r.pillars.len() == 2),
            "every row carries the spine"
        );
        let row = &terrain.rows[3];
        assert!(
            (8..W - 8).contains(&row.pillars[0]),
            "and it runs down the middle of the court"
        );
    }

    #[test]
    fn maze_walls_alternate_sides() {
        let terrain = Terrain::new(TerrainKind::Maze, 4);
        assert!(
            terrain.rows.iter().any(|r| r.open.0 > 1),
            "some blocks wall the left"
        );
        assert!(
            terrain.rows.iter().any(|r| r.open.1 < W - 2),
            "some wall the right"
        );
        assert!(
            terrain.rows.iter().any(|r| r.open == (1, W - 2)),
            "and there is clear water between them"
        );
    }

    #[test]
    fn a_reef_is_thicker_than_a_cave() {
        assert!(
            count_pillars(TerrainKind::Reef) > count_pillars(TerrainKind::Cave),
            "the reef is the one that is wall to wall"
        );
        assert_eq!(count_pillars(TerrainKind::Canyon), 0, "a canyon is clear");
    }

    #[test]
    fn the_new_sectors_bring_their_own_trouble() {
        let hazards = |sector: Sector| {
            let mut g = flying();
            g.node = RouteNode {
                sector,
                terrain: TerrainKind::Open,
                bonus: NodeBonus::Refit,
            };
            g.spawn_wave();
            g
        };
        let corona = hazards(Sector::SolarCorona);
        assert_eq!(
            corona
                .hazards
                .iter()
                .filter(|h| matches!(h, Hazard::SolarFlare { .. }))
                .count(),
            2,
            "the corona sweeps two walls of fire"
        );
        let void = hazards(Sector::VoidRift);
        assert_eq!(
            void.hazards
                .iter()
                .filter(|h| matches!(h, Hazard::GravityWell { .. }))
                .count(),
            2,
            "the rift drags from two wells"
        );
        let comet = hazards(Sector::CometTrail);
        assert!(
            comet
                .hazards
                .iter()
                .any(|h| matches!(h, Hazard::IonStream { .. })),
            "the comet trail runs a current"
        );
        assert!(
            Sector::CometTrail.asteroid_chance() < Sector::AsteroidBelt.asteroid_chance(),
            "and throws more rock than the belt"
        );
        let wreck = hazards(Sector::Wreckage);
        assert!(
            wreck.debris.len() > 8 && !wreck.mines.is_empty(),
            "the graveyard is full of hulks and old mines"
        );
    }

    #[test]
    fn every_map_can_be_flown_without_panicking() {
        for terrain in TerrainKind::ALL {
            for sector in Sector::ALL {
                let mut g = Game::new(5);
                g.start(ShipClass::Cruiser, Difficulty::Normal);
                g.node = RouteNode {
                    sector,
                    terrain,
                    bonus: NodeBonus::Refit,
                };
                g.spawn_wave();
                for i in 0..300 {
                    if i % 3 == 0 {
                        g.fire();
                    }
                    g.move_ship(if i % 7 < 3 { 1 } else { -1 }, 0);
                    g.step();
                }
                assert_eq!(
                    g.terrain.rows.len(),
                    H as usize,
                    "{} in {} keeps its rock",
                    terrain.name(),
                    sector.name()
                );
            }
        }
    }
}
