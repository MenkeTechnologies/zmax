//! Nova — the heavy-weapons formation shooter.
//!
//! Where `galaga` is the plain arcade original, Nova is a full shoot-'em-up
//! campaign on the same court: you build a ship, fly sectors that each fight
//! differently, level a pilot up and spend salvage in a hangar between waves.
//!
//! * **Fighters** — A-wing (fastest thing in the fleet, boosts across the
//!   court), X-wing (the workhorse, deflectors that eat fire), Y-wing (armoured
//!   bomber, proton salvo), B-wing (the hardest-hitting cannons) and a Corellian
//!   freighter (five shield pips and a full bomb bay). Every one carries a
//!   special paid for out of an energy meter.
//! * **Ship building** — five components (engine, reactor, plating, cannon,
//!   magazine) upgrade through four tiers, and five modules (magnet, autoloader,
//!   salvager, repair bay, overdrive) bolt on permanently. Everything is bought
//!   in the hangar between waves with salvage picked up from kills.
//! * **Guns** — twelve of them, three levels each, with wing drones firing
//!   alongside: blaster, spread, piercing laser, homing missiles, wide plasma,
//!   the vulcan machine gun, dumb-fire rockets that blast a hole where they
//!   land, flak shells that burst into a fan, a rail slug that runs the whole
//!   court, an arc bolt that earths itself through a crowd, proton torpedoes
//!   that take an emplacement off a capital hull in one go, and an ion cannon
//!   that scrambles emplacements rather than breaking them — a scrambled dome
//!   is a hull that can be shot.
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
//! Controls in flight: `←/→`/`h`/`l` and `↑/↓`/`k`/`j` fly, `SPC` (or `f`)
//! fires, `m` throws missiles, `1`-`0` and `[`/`]` swap guns, `x` triggers the
//! hull special, `b` drops a smart bomb, `p` pauses, `r` retries with the same
//! build, `n` restarts, `q`/`Esc` quits. The picker takes `1`/`2`/`3` or `←/→`
//! for the hull, `d` for difficulty, `g` for the galaxy, `Enter` to launch. In
//! the hangar the listed key buys the line, `w` climbs into the next hull and
//! `Enter` opens the chart; on the chart `←/→` picks a lane and `Enter` flies
//! it.
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
/// Ticks an ion hit scrambles an emplacement for.
const ION_STUN: u32 = 90;
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
/// The Force builds as you fly and is spent on the three things a pilot who
/// trusts it can do: stretch out with his senses, pull what he needs to him,
/// and let go of the targeting computer.
const FORCE_MAX: u32 = 100;
const FORCE_PER_KILL: u32 = 6;
const FORCE_REGEN_TICKS: u32 = 20;
const SENSE_COST: u32 = 45;
const SENSE_TICKS: u32 = 90;
const PULL_COST: u32 = 30;
const GUIDED_COST: u32 = 60;
/// The score that gets a bounty posted on you, and what his ship is worth.
const HUNTER_BOUNTY: u32 = 20_000;
/// Footing each side brings to a duel, how long a guard holds, how wide the
/// parry window is, how long a riposte stays open and what a lock costs.
const DUEL_POISE: i32 = 5;
/// How close a blade has to be before it is a duel.
const DUEL_RANGE: i16 = 3;
const GUARD_TICKS: u32 = 5;
const PARRY_WINDOW: u32 = 4;
const RIPOSTE_TICKS: u32 = 8;
const LOCK_TICKS: u32 = 12;
const STAGGER_TICKS: u32 = 18;
/// What a Force shove costs, and how far it throws somebody.
const PUSH_COST: u32 = 25;
const PUSH_REACH: i16 = 4;
/// What a sabre does, how fast it cuts and how far it reaches; how fast a scout
/// walker and a duellist work; and what a speeder costs you if you hit a tree.
const SABRE_DAMAGE: i32 = 6;
const SABRE_CADENCE: u32 = 4;
const SABRE_REACH: i16 = 2;
const SCOUT_CADENCE: u32 = 30;
const DUEL_CADENCE: u32 = 10;
const SPEEDER_PACE: i16 = 3;
const SPEEDER_CRASH: i32 = 3;
/// What a pilot can take on foot, how fast his sidearm cycles, how fast a
/// patrol answers, and how far a blaster bolt carries.
const PILOT_HEALTH: i32 = 12;
const BLASTER_CADENCE: u32 = 3;
const TROOPER_CADENCE: u32 = 22;
const BOLT_RANGE: u32 = 18;
/// Ticks between the Alliance cruiser's salvoes, and what they hit for.
const ALLY_CADENCE: u32 = 18;
const ALLY_DAMAGE: i32 = 6;
/// Ticks between a walker's shots, and turns of cable it takes to bring one down.
const WALKER_CADENCE: u32 = 26;
const CABLE_WRAPS: u32 = 2;
/// Ticks a line of radio chatter stays on the display.
const CHATTER_TICKS: u32 = 110;
/// Pips of power the reactor splits between lasers, shields and engines, and
/// how long a fully-charged shield takes to knit a pip back.
const POWER_PIPS: u32 = 6;
const SHIELD_KNIT_TICKS: u32 = 600;
/// Hulls a squad can hold, where the wingmen ride, how often they fire, and
/// what a new hull or a rescue costs.
const MAX_SQUAD: usize = 6;
const WING_SLOTS: usize = MAX_SQUAD - 1;
const WING_CADENCE: u32 = 9;
/// Missiles the launcher fires per salvo, and what it holds to start with.
const MISSILE_SALVO: u32 = 2;
const MISSILE_START: u32 = 6;
const MISSILE_PACK: u32 = 8;
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
/// Ticks the arrival banner stays up for.
const BANNER_TICKS: u32 = 40;
/// Rows of parallax backdrop drawn behind the court.
const STAR_LAYERS: usize = 3;
/// Segments a serpent boss trails behind its head.
const SERPENT_SEGMENTS: usize = 8;
/// The vertical offsets a serpent's body cycles through as it swims.
const SERPENT_WAVE: [i16; 8] = [0, 1, 2, 1, 0, -1, -2, -1];
/// The keys the hangar hands out to its lines, in order.
const SHOP_KEYS: [char; 20] = [
    '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'c', 'e', 'g', 'i', 'k', 'm', 'o', 's', 'u',
    'y',
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
    Boost,
    /// A bubble that eats every shot that reaches the hull for a while.
    Deflectors,
    /// A wall of bolts laid across the whole court at once.
    ProtonSalvo,
}

impl Special {
    pub fn name(self) -> &'static str {
        match self {
            Special::Boost => "engine boost",
            Special::Deflectors => "deflectors",
            Special::ProtonSalvo => "proton salvo",
        }
    }
}

/// The three hulls, trading speed and rate of fire against armour and damage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShipClass {
    AWing,
    XWing,
    YWing,
    BWing,
    Freighter,
}

impl ShipClass {
    pub const ALL: [ShipClass; 5] = [
        ShipClass::AWing,
        ShipClass::XWing,
        ShipClass::YWing,
        ShipClass::BWing,
        ShipClass::Freighter,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ShipClass::AWing => "A-wing",
            ShipClass::XWing => "X-wing",
            ShipClass::YWing => "Y-wing",
            ShipClass::BWing => "B-wing",
            ShipClass::Freighter => "Corellian freighter",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            ShipClass::AWing => "▴",
            ShipClass::XWing => "✕",
            ShipClass::YWing => "⋏",
            ShipClass::BWing => "⌖",
            ShipClass::Freighter => "◙",
        }
    }

    /// The hull close up: five rows of it, for the views where a fighter fills
    /// the screen. Drawn nose-on, the way you see one parked in a bay.
    pub fn model(self) -> [&'static str; 5] {
        match self {
            ShipClass::AWing => ["   ▲   ", "  ▟█▙  ", " ▟███▙ ", "▟█████▙", " ╘═╧═╛ "],
            ShipClass::XWing => ["╲     ╱", " ╲▁█▁╱ ", "  ███  ", " ╱▔█▔╲ ", "╱     ╲"],
            ShipClass::YWing => ["┏━┓ ┏━┓", "┃█┃ ┃█┃", "┗┳┛█┗┳┛", " ┃███┃ ", " ╹╹ ╹╹ "],
            ShipClass::BWing => ["   ┃   ", "   ┃   ", "━━█████", "   ┃   ", "   ╹   "],
            ShipClass::Freighter => [" ╭───╮ ", "╭┤███├╮", "│█◙█◙█│", "╰┤███├╯", " ╰───╯ "],
        }
    }

    /// The hull as it is actually drawn: three cells across, two rows deep,
    /// centred on the cell the game tracks. An X-wing has its S-foils out, a
    /// Y-wing its engine nacelles, a B-wing its cross, and so on.
    pub fn sprite(self) -> [&'static str; 2] {
        match self {
            ShipClass::AWing => [" ▲ ", "╘═╛"],
            ShipClass::XWing => ["╲▲╱", "╱█╲"],
            ShipClass::YWing => ["┳▲┳", "╹█╹"],
            ShipClass::BWing => [" ┃ ", "━█━"],
            ShipClass::Freighter => ["╭◙╮", "╰─╯"],
        }
    }

    /// Columns the hull slides per keypress before the engine is upgraded.
    pub fn speed(self) -> i16 {
        match self {
            ShipClass::AWing | ShipClass::Freighter => 2,
            _ => 1,
        }
    }

    /// Shield pips it soaks before a hit costs a life.
    pub fn max_shield(self) -> u32 {
        match self {
            ShipClass::AWing => 1,
            ShipClass::XWing => 2,
            ShipClass::BWing => 3,
            ShipClass::YWing => 4,
            ShipClass::Freighter => 5,
        }
    }

    /// Ticks between shots before the magazine is upgraded.
    pub fn fire_cadence(self) -> u32 {
        match self {
            ShipClass::AWing | ShipClass::Freighter => 2,
            ShipClass::XWing | ShipClass::BWing => 3,
            ShipClass::YWing => 4,
        }
    }

    /// Damage each of its shots carries at gun level one.
    pub fn damage(self) -> i32 {
        match self {
            ShipClass::AWing => 1,
            ShipClass::XWing | ShipClass::Freighter => 2,
            ShipClass::YWing => 3,
            ShipClass::BWing => 4,
        }
    }

    /// Smart bombs it launches with.
    pub fn bombs(self) -> u32 {
        match self {
            ShipClass::AWing => 2,
            ShipClass::XWing | ShipClass::BWing => 3,
            ShipClass::YWing => 4,
            ShipClass::Freighter => 5,
        }
    }

    pub fn special(self) -> Special {
        match self {
            ShipClass::AWing => Special::Boost,
            ShipClass::XWing | ShipClass::Freighter => Special::Deflectors,
            ShipClass::YWing | ShipClass::BWing => Special::ProtonSalvo,
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            ShipClass::AWing => "fastest thing in the fleet, no armour; boosts across the court",
            ShipClass::XWing => "the workhorse: S-foils locked, deflectors that eat fire",
            ShipClass::YWing => "slow armoured bomber, four shield pips; lays a proton salvo",
            ShipClass::BWing => "heavy assault: the hardest-hitting cannons in the fleet",
            ShipClass::Freighter => {
                "a light freighter with a full bomb bay; she has it where it counts"
            }
        }
    }
}

/// Imperial hardware, as it is actually drawn.
impl EnemyKind {
    /// Three cells of hull, centred on the cell it flies in.
    pub fn sprite(self) -> &'static str {
        match self {
            EnemyKind::TieFighter => "|●|",
            EnemyKind::TieInterceptor => "/●\\",
            EnemyKind::TieBomber => "[●]",
            EnemyKind::TieDefender => "⟨●⟩",
            EnemyKind::TieAdvanced => "«●»",
            EnemyKind::GunPlatform => "╪⊕╪",
            EnemyKind::Gunboat => "▐Ѫ▌",
            EnemyKind::MineLayer => "◄Ѳ►",
            EnemyKind::VultureDroid => "≺Ж≻",
            EnemyKind::BuzzDroid => " ѵ ",
            EnemyKind::RepairDroid => " ✚ ",
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
    /// How many lanes a jump can cross at once.
    Hyperdrive,
}

impl Part {
    pub const ALL: [Part; 6] = [
        Part::Engine,
        Part::Reactor,
        Part::Plating,
        Part::Cannon,
        Part::Magazine,
        Part::Hyperdrive,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Part::Engine => "sublight engines",
            Part::Reactor => "power core",
            Part::Plating => "deflector plating",
            Part::Cannon => "laser cannons",
            Part::Magazine => "capacitors",
            Part::Hyperdrive => "hyperdrive",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Part::Engine => "+1 column of thrust every two tiers",
            Part::Reactor => "+1 energy per tick and +10 capacity per tier",
            Part::Plating => "+1 shield pip per tier",
            Part::Cannon => "+1 damage per tier",
            Part::Magazine => "-1 tick between shots every two tiers",
            Part::Hyperdrive => "+1 lane of jump range every two tiers",
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
            Module::Magnet => "magnetic scoop",
            Module::Autoloader => "servo loader",
            Module::Salvager => "salvage droid",
            Module::RepairBay => "R-unit astromech",
            Module::Overdrive => "power converter",
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
    LaserCannon,
    QuadLaser,
    HeavyLaser,
    ConcussionMissile,
    ProtonBomb,
    /// A stuttering machine gun: little rounds, almost no gap between them.
    RepeatingBlaster,
    /// Dumb-fire rockets that blow a hole where they land.
    RocketPod,
    /// Shells that burst into a fan of fragments part way up the court.
    Flechette,
    /// One slow, enormous piercing slug.
    MassDriver,
    /// A bolt that jumps from hull to hull.
    ArcCaster,
    /// Proton torpedoes: slow, few, and they take a battery off a capital hull
    /// in one go.
    ProtonTorpedo,
    /// An ion cannon: it scrambles emplacements instead of breaking them, and a
    /// scrambled shield dome is a hull that can be shot.
    IonCannon,
    /// A tow cable: useless against fighters, and the only thing that will put
    /// a walker on its side.
    TowCable,
}

impl Weapon {
    pub const ALL: [Weapon; 13] = [
        Weapon::LaserCannon,
        Weapon::QuadLaser,
        Weapon::HeavyLaser,
        Weapon::ConcussionMissile,
        Weapon::ProtonBomb,
        Weapon::RepeatingBlaster,
        Weapon::RocketPod,
        Weapon::Flechette,
        Weapon::MassDriver,
        Weapon::ArcCaster,
        Weapon::ProtonTorpedo,
        Weapon::IonCannon,
        Weapon::TowCable,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Weapon::LaserCannon => "laser cannon",
            Weapon::QuadLaser => "quad laser",
            Weapon::HeavyLaser => "heavy laser",
            Weapon::ConcussionMissile => "concussion missile",
            Weapon::ProtonBomb => "proton bomb",
            Weapon::RepeatingBlaster => "repeating blaster",
            Weapon::RocketPod => "rocket pod",
            Weapon::Flechette => "flechette",
            Weapon::MassDriver => "mass driver",
            Weapon::ArcCaster => "arc caster",
            Weapon::ProtonTorpedo => "proton torpedo",
            Weapon::IonCannon => "ion cannon",
            Weapon::TowCable => "tow cable",
        }
    }

    /// Ticks added to (or shaved off) the hull's firing cadence: a vulcan
    /// hoses, a rail gun takes its time.
    pub fn cadence_shift(self) -> i32 {
        match self {
            Weapon::RepeatingBlaster => -2,
            Weapon::LaserCannon | Weapon::QuadLaser => 0,
            Weapon::HeavyLaser | Weapon::ArcCaster => 1,
            Weapon::ConcussionMissile | Weapon::ProtonBomb | Weapon::Flechette => 2,
            Weapon::RocketPod | Weapon::IonCannon => 3,
            Weapon::TowCable => 5,
            Weapon::ProtonTorpedo => 8,
            Weapon::MassDriver => 6,
        }
    }

    /// The single letter its pickup shows on the court.
    pub fn tag(self) -> &'static str {
        match self {
            Weapon::LaserCannon => "B",
            Weapon::QuadLaser => "S",
            Weapon::HeavyLaser => "L",
            Weapon::ConcussionMissile => "H",
            Weapon::ProtonBomb => "P",
            Weapon::RepeatingBlaster => "V",
            Weapon::RocketPod => "R",
            Weapon::Flechette => "F",
            Weapon::MassDriver => "X",
            Weapon::ArcCaster => "A",
            Weapon::ProtonTorpedo => "T",
            Weapon::IonCannon => "I",
            Weapon::TowCable => "C",
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
    Cable,
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
            ShotKind::Cable => "═",
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
    /// Ion rounds scramble what they hit rather than breaking it.
    pub ion: bool,
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
            ion: false,
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

    /// A proton torpedo: slow, heavy, and it takes a whole emplacement with it.
    fn torpedo(pos: (i16, i16), damage: i32) -> Shot {
        Shot {
            speed: -1,
            splash: 1,
            homing: true,
            kind: ShotKind::Rocket,
            ..Shot::bolt(pos, 0, damage)
        }
    }

    /// An ion bolt: it scrambles emplacements and gun crews for a while.
    fn ion(pos: (i16, i16), damage: i32) -> Shot {
        Shot {
            speed: -2,
            splash: 1,
            ion: true,
            kind: ShotKind::Arc,
            ..Shot::bolt(pos, 0, damage)
        }
    }

    /// A tow cable: it sweeps out and wraps whatever it drags across.
    fn cable(pos: (i16, i16), drift: i16) -> Shot {
        Shot {
            speed: -1,
            pierce: true,
            kind: ShotKind::Cable,
            ..Shot::bolt(pos, drift, 1)
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
    TieFighter,
    TieInterceptor,
    GunPlatform,
    TieBomber,
    BuzzDroid,
    Gunboat,
    /// Telegraphs, then throws a shot twice as fast as anything else.
    TieDefender,
    /// Leaves mines hanging in the court behind it.
    MineLayer,
    /// Breaks into two diving grunts when it dies.
    VultureDroid,
    /// Repairs damaged hulls flying near it.
    RepairDroid,
    /// A capital ship's fighter screen: fast, twitchy, fires in pairs.
    TieAdvanced,
}

impl EnemyKind {
    pub fn hp(self) -> i32 {
        match self {
            EnemyKind::TieFighter | EnemyKind::BuzzDroid => 1,
            EnemyKind::TieInterceptor | EnemyKind::TieDefender | EnemyKind::TieAdvanced => 2,
            EnemyKind::GunPlatform
            | EnemyKind::MineLayer
            | EnemyKind::VultureDroid
            | EnemyKind::RepairDroid => 3,
            EnemyKind::TieBomber => 4,
            EnemyKind::Gunboat => 7,
        }
    }

    pub fn score(self) -> u32 {
        match self {
            EnemyKind::TieFighter => 10,
            EnemyKind::TieInterceptor => 20,
            EnemyKind::BuzzDroid => 25,
            EnemyKind::TieAdvanced => 30,
            EnemyKind::GunPlatform => 30,
            EnemyKind::MineLayer => 35,
            EnemyKind::TieBomber => 40,
            EnemyKind::VultureDroid => 40,
            EnemyKind::TieDefender => 45,
            EnemyKind::RepairDroid => 50,
            EnemyKind::Gunboat => 80,
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            EnemyKind::TieFighter => "ᴥ",
            EnemyKind::TieInterceptor => "ʬ",
            EnemyKind::GunPlatform => "⊕",
            EnemyKind::TieBomber => "҂",
            EnemyKind::BuzzDroid => "ѵ",
            EnemyKind::Gunboat => "Ѫ",
            EnemyKind::TieDefender => "⌖",
            EnemyKind::MineLayer => "Ѳ",
            EnemyKind::VultureDroid => "Ж",
            EnemyKind::RepairDroid => "✚",
            EnemyKind::TieAdvanced => "Ѭ",
        }
    }

    /// 1-in-N chance per tick of peeling out of formation; `0` never dives.
    fn dive_chance(self) -> u64 {
        match self {
            EnemyKind::TieFighter => 140,
            EnemyKind::BuzzDroid => 70,
            EnemyKind::TieBomber => 220,
            EnemyKind::VultureDroid => 160,
            EnemyKind::TieAdvanced => 50,
            _ => 0,
        }
    }

    /// 1-in-N chance per tick of shooting (or, for a miner, of dropping a
    /// mine); `0` never does.
    fn fire_chance(self) -> u64 {
        match self {
            EnemyKind::TieFighter => 180,
            EnemyKind::GunPlatform => 60,
            EnemyKind::TieBomber => 90,
            EnemyKind::Gunboat => 120,
            EnemyKind::TieDefender => 100,
            EnemyKind::MineLayer => 150,
            EnemyKind::TieAdvanced => 70,
            _ => 0,
        }
    }

    /// Rows a diving hull of this kind covers per tick.
    fn dive_speed(self) -> i16 {
        match self {
            EnemyKind::BuzzDroid | EnemyKind::TieAdvanced => 2,
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
            Sector::OpenSpace => "deep space",
            Sector::AsteroidBelt => "Hoth asteroid field",
            Sector::Nebula => "Kessel nebula",
            Sector::Minefield => "Imperial minefield",
            Sector::IonStorm => "ion storm",
            Sector::DebrisRing => "Alderaan debris",
            Sector::SolarCorona => "Tatooine twin suns",
            Sector::Wreckage => "Jakku graveyard",
            Sector::CometTrail => "Bespin gas streams",
            Sector::VoidRift => "Maw cluster",
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
    /// A pack for the missile launcher.
    Missiles,
    /// An escape pod: a pilot back, and salvage with him.
    EscapePod,
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
            PowerKind::Missiles => "↥",
            PowerKind::EscapePod => "☉",
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
    /// A TIE Advanced x1 flown by somebody who knows how: it jinks, it leads
    /// the hull, and it breaks off rather than die.
    AceTie,
}

impl BossKind {
    /// The bosses cycle in this order, one per boss wave.
    pub fn of_wave(wave: u32) -> BossKind {
        match (wave / BOSS_EVERY) % 5 {
            1 => BossKind::AceTie,
            2 => BossKind::Twin,
            3 => BossKind::Carrier,
            4 => BossKind::Serpent,
            _ => BossKind::Dreadnought,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            BossKind::Dreadnought => "dreadnought",
            BossKind::Twin => "twin",
            BossKind::Carrier => "carrier",
            BossKind::Serpent => "serpent",
            BossKind::AceTie => "TIE Advanced x1",
        }
    }

    /// Half-width of the core hull.
    pub fn core_half(self) -> i16 {
        match self {
            BossKind::Twin => 3,
            BossKind::Serpent | BossKind::AceTie => 1,
            _ => 6,
        }
    }

    /// Extra rows the core hull covers below its anchor row.
    pub fn core_depth(self) -> i16 {
        match self {
            BossKind::Serpent | BossKind::AceTie => 0,
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
            BossKind::AceTie => Vec::new(),
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
            (BossKind::AceTie, 3) => 3,
            (BossKind::AceTie, _) => 2,
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
    /// Parked at a system, reading the galaxy chart.
    Chart,
    /// Down on a world, out of the cockpit and on foot.
    Surface,
    /// The war is won: the ceremony at the base.
    Ceremony,
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
    /// Another hull for the squad, flown by a wingman.
    Hull,
    /// Every downed wingman back in the air.
    Rescue,
    /// A pack for the missile launcher.
    Missiles,
}

impl Stock {
    pub const ALL: [Stock; 10] = [
        Stock::Repair,
        Stock::Missiles,
        Stock::GunLevel,
        Stock::GunSwap,
        Stock::Drone,
        Stock::Bomb,
        Stock::Rapid,
        Stock::Rescue,
        Stock::Hull,
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
            Stock::Life => "spare life",
            Stock::Hull => "another fighter",
            Stock::Rescue => "recover the wing",
            Stock::Missiles => "torpedo pack",
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
            Stock::Hull => "another hull for the squad, flown as a wingman",
            Stock::Rescue => "every downed wingman back in the air",
            Stock::Missiles => "+8 rounds for the launcher",
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
            Stock::Missiles => 350,
            Stock::Rescue => 700,
            Stock::Hull => 2500,
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
    /// A capital ship's surface trench: two walls, one lane, and the guns are
    /// bolted to both sides of it.
    Trench,
    /// The forest floor: trunks everywhere and barely a lane between them.
    Forest,
}

impl TerrainKind {
    pub const ALL: [TerrainKind; 11] = [
        TerrainKind::Open,
        TerrainKind::Canyon,
        TerrainKind::Cave,
        TerrainKind::Tunnel,
        TerrainKind::Pillars,
        TerrainKind::Gates,
        TerrainKind::Spine,
        TerrainKind::Maze,
        TerrainKind::Reef,
        TerrainKind::Trench,
        TerrainKind::Forest,
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
            TerrainKind::Trench => "trench",
            TerrainKind::Forest => "forest",
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
            TerrainKind::Trench => "one lane between two armoured walls",
            TerrainKind::Forest => "trunks everywhere; fly it at speed and see",
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
            TerrainKind::Trench => (18, 22),
            TerrainKind::Forest => (W - 10, W - 4),
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
            TerrainKind::Trench => 1,
            TerrainKind::Forest => 2,
        }
    }

    /// Whether shooting the rock carves it away.
    fn destructible(self) -> bool {
        matches!(
            self,
            TerrainKind::Cave
                | TerrainKind::Pillars
                | TerrainKind::Reef
                | TerrainKind::Spine
                | TerrainKind::Forest
        )
    }

    /// 1-in-N odds a generated row grows a rock column in the channel.
    fn pillar_chance(self) -> u64 {
        match self {
            TerrainKind::Forest => 2,
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

/// The galaxy a run is flown in. Each one fields a different mix of sectors,
/// rock and enemies, and pays differently for it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Galaxy {
    /// The starting arm: everything shows up, nothing is extreme.
    Orion,
    /// Swarm space: deeper formations, thinner armour, more drops.
    Hive,
    /// Industrial space: hulks, mines and turrets, but the salvage is rich.
    Forge,
    /// Burnt space: flares, corona and rock, with heavier hulls.
    Cinder,
    /// The deep: wells, void and long tunnels, and everything is armoured.
    Abyss,
}

impl Galaxy {
    pub const ALL: [Galaxy; 5] = [
        Galaxy::Orion,
        Galaxy::Hive,
        Galaxy::Forge,
        Galaxy::Cinder,
        Galaxy::Abyss,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Galaxy::Orion => "Outer Rim Territories",
            Galaxy::Hive => "Kessel Sector",
            Galaxy::Forge => "Corellian Run",
            Galaxy::Cinder => "Tatooine Sector",
            Galaxy::Abyss => "The Maw",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Galaxy::Orion => "the frontier: a bit of everything, nothing extreme",
            Galaxy::Hive => "spice lanes thick with TIEs, thin hulls, rich salvage",
            Galaxy::Forge => "shipyards and minefields — and heavy salvage",
            Galaxy::Cinder => "twin suns and rock; the patrols out here are tough",
            Galaxy::Abyss => "black holes and long tunnels; everything is armoured",
        }
    }

    /// Extra rows of hulls a wave fields here.
    pub fn swarm(self) -> usize {
        match self {
            Galaxy::Hive => 1,
            _ => 0,
        }
    }

    /// Extra armour on every enemy hull.
    pub fn armour(self) -> i32 {
        match self {
            Galaxy::Hive => -1,
            Galaxy::Cinder => 1,
            Galaxy::Abyss => 2,
            _ => 0,
        }
    }

    /// Percentage on top of every salvage payout.
    pub fn salvage_bonus(self) -> u32 {
        match self {
            Galaxy::Forge => 60,
            Galaxy::Abyss => 30,
            Galaxy::Hive => 15,
            _ => 0,
        }
    }

    /// The sectors that show up in this galaxy.
    pub fn sectors(self) -> &'static [Sector] {
        match self {
            Galaxy::Orion => &Sector::ALL,
            Galaxy::Hive => &[
                Sector::OpenSpace,
                Sector::Nebula,
                Sector::CometTrail,
                Sector::IonStorm,
            ],
            Galaxy::Forge => &[
                Sector::Wreckage,
                Sector::DebrisRing,
                Sector::Minefield,
                Sector::AsteroidBelt,
            ],
            Galaxy::Cinder => &[
                Sector::SolarCorona,
                Sector::AsteroidBelt,
                Sector::OpenSpace,
                Sector::IonStorm,
            ],
            Galaxy::Abyss => &[
                Sector::VoidRift,
                Sector::Nebula,
                Sector::Minefield,
                Sector::DebrisRing,
            ],
        }
    }

    /// The rock that shows up in this galaxy.
    pub fn terrains(self) -> &'static [TerrainKind] {
        match self {
            Galaxy::Orion => &TerrainKind::ALL,
            Galaxy::Hive => &[
                TerrainKind::Open,
                TerrainKind::Pillars,
                TerrainKind::Canyon,
                TerrainKind::Gates,
            ],
            Galaxy::Forge => &[
                TerrainKind::Reef,
                TerrainKind::Maze,
                TerrainKind::Spine,
                TerrainKind::Pillars,
            ],
            Galaxy::Cinder => &[
                TerrainKind::Canyon,
                TerrainKind::Cave,
                TerrainKind::Open,
                TerrainKind::Spine,
            ],
            Galaxy::Abyss => &[
                TerrainKind::Tunnel,
                TerrainKind::Maze,
                TerrainKind::Cave,
                TerrainKind::Gates,
            ],
        }
    }

    /// The grid its chart is laid out on, as `(columns, rows)`. A galaxy is a
    /// map to roam, not a corridor: even the smallest runs to sixty systems.
    pub fn chart(self) -> (usize, usize) {
        match self {
            Galaxy::Orion => (14, 7),
            Galaxy::Hive => (15, 8),
            Galaxy::Forge => (13, 7),
            Galaxy::Cinder => (14, 8),
            Galaxy::Abyss => (16, 9),
        }
    }

    /// Roughly how many systems that grid holds once it is scattered.
    pub fn systems(self) -> usize {
        let (w, h) = self.chart();
        w * h * 7 / 10
    }
}

/// What is waiting at a system on the chart.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    /// An ordinary wave.
    Battle,
    /// A wave with heavier hulls and a better payout.
    Elite,
    /// A boss.
    Boss,
    /// No fight: a hangar with a free repair.
    Depot,
    /// No fight: a hulk to strip for salvage and a gun.
    Derelict,
    /// A capital ship: a wedge destroyer holding the system, or a battlestation
    /// with a trench to run.
    Capital,
}

impl NodeKind {
    pub fn name(self) -> &'static str {
        match self {
            NodeKind::Battle => "patrol",
            NodeKind::Elite => "elite squadron",
            NodeKind::Boss => "ace pilot",
            NodeKind::Depot => "Rebel outpost",
            NodeKind::Derelict => "wrecked freighter",
            NodeKind::Capital => "capital ship",
        }
    }

    /// The glyph the chart draws it with.
    pub fn glyph(self) -> &'static str {
        match self {
            NodeKind::Battle => "◇",
            NodeKind::Elite => "◆",
            NodeKind::Boss => "☠",
            NodeKind::Depot => "⌂",
            NodeKind::Derelict => "⌗",
            NodeKind::Capital => "▰",
        }
    }

    /// Whether flying here means a fight.
    pub fn fights(self) -> bool {
        matches!(
            self,
            NodeKind::Battle | NodeKind::Elite | NodeKind::Boss | NodeKind::Capital
        )
    }
}

/// One system on the chart.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapNode {
    /// Where it sits on the chart, as `(column, row)`.
    pub pos: (usize, usize),
    /// The band of the galaxy it stands in.
    pub region: Region,
    pub kind: NodeKind,
    pub sector: Sector,
    pub terrain: TerrainKind,
    pub bonus: NodeBonus,
    pub cleared: bool,
    /// True once the chart has seen it.
    pub explored: bool,
}

impl MapNode {
    pub fn label(&self) -> String {
        format!(
            "{} · {} · {} · {} — {}",
            self.kind.name(),
            self.region.name(),
            self.sector.name(),
            self.terrain.name(),
            self.bonus.label()
        )
    }
}

/// A band of the galaxy, from the quiet rim in to the deep.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Region {
    Rim,
    Verge,
    Reach,
    Core,
    Deep,
}

impl Region {
    /// Which band a column of the chart falls in.
    pub fn of_column(col: usize, width: usize) -> Region {
        match col * 5 / width.max(1) {
            0 => Region::Rim,
            1 => Region::Verge,
            2 => Region::Reach,
            3 => Region::Core,
            _ => Region::Deep,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Region::Rim => "the Outer Rim",
            Region::Verge => "the Mid Rim",
            Region::Reach => "the Expansion Region",
            Region::Core => "the Inner Rim",
            Region::Deep => "the Core Worlds",
        }
    }

    /// Extra armour every hull in this band carries.
    pub fn armour(self) -> i32 {
        match self {
            Region::Rim => 0,
            Region::Verge => 1,
            Region::Reach => 2,
            Region::Core => 3,
            Region::Deep => 5,
        }
    }
}

/// The galaxy chart: a grid of systems joined by lanes, most of it dark until
/// it is flown. Lanes run both ways and neighbours link up in four directions,
/// so the squad roams rather than following a corridor.
#[derive(Clone, Debug)]
pub struct StarMap {
    pub galaxy: Galaxy,
    pub nodes: Vec<MapNode>,
    /// `grid[row][col]` is the system standing there, if any.
    pub grid: Vec<Vec<Option<usize>>>,
    pub lanes: Vec<Vec<usize>>,
    /// Which system the squad is parked at.
    pub at: usize,
    /// Which reachable system the chart cursor is on.
    pub cursor: usize,
    pub width: usize,
    pub height: usize,
}

impl StarMap {
    /// Lay out a galaxy: a grid of systems, most cells filled, every one linked
    /// to the neighbours it has, plus a few hyperlanes across the map. What
    /// stands in a system gets nastier the deeper into the galaxy it is.
    pub fn generate(galaxy: Galaxy, seed: u64) -> StarMap {
        let mut rng = seed | 1;
        let mut rand = move || {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            rng >> 33
        };
        let (width, height) = galaxy.chart();
        let sectors = galaxy.sectors();
        let terrains = galaxy.terrains();
        let mut nodes: Vec<MapNode> = Vec::new();
        let mut grid = vec![vec![None; width]; height];
        for col in 0..width {
            for row in 0..height {
                // The rim column is solid so the run always has somewhere to
                // start; everything past it is scattered.
                let filled = col == 0 || rand() % 10 < 7;
                if !filled {
                    continue;
                }
                let region = Region::of_column(col, width);
                let kind = if col == 0 {
                    if row == height / 2 {
                        NodeKind::Depot
                    } else {
                        NodeKind::Battle
                    }
                } else {
                    match rand() % 24 {
                        0..=1 => NodeKind::Depot,
                        2 => NodeKind::Derelict,
                        3..=5 => NodeKind::Elite,
                        6 if region >= Region::Reach => NodeKind::Capital,
                        7 if region >= Region::Core => NodeKind::Capital,
                        8 if region >= Region::Verge => NodeKind::Boss,
                        _ => NodeKind::Battle,
                    }
                };
                let sector = sectors[(rand() % sectors.len() as u64) as usize];
                let terrain = if kind == NodeKind::Capital {
                    // A capital ship is fought over open plating or down a
                    // trench, never in a cave.
                    if rand() % 2 == 0 {
                        TerrainKind::Trench
                    } else {
                        TerrainKind::Open
                    }
                } else {
                    terrains[(rand() % terrains.len() as u64) as usize]
                };
                let bonus = match rand() % 4 {
                    0 => NodeBonus::Cache(300 + 120 * col as u32),
                    1 => NodeBonus::Armoury(
                        Weapon::ALL[(rand() % Weapon::ALL.len() as u64) as usize],
                    ),
                    2 => NodeBonus::Refit,
                    _ => NodeBonus::Danger,
                };
                grid[row][col] = Some(nodes.len());
                nodes.push(MapNode {
                    pos: (col, row),
                    region,
                    kind,
                    sector,
                    terrain,
                    bonus,
                    cleared: false,
                    explored: false,
                });
            }
        }
        let mut lanes = vec![Vec::new(); nodes.len()];
        let link = |lanes: &mut Vec<Vec<usize>>, a: usize, b: usize| {
            if a != b && !lanes[a].contains(&b) {
                lanes[a].push(b);
                lanes[b].push(a);
            }
        };
        for row in 0..height {
            for col in 0..width {
                let Some(here) = grid[row][col] else {
                    continue;
                };
                if col + 1 < width {
                    // The lane ahead, or the nearest diagonal when the cell
                    // straight on is empty space.
                    for (dr, dc) in [(0i32, 1i32), (-1, 1), (1, 1)] {
                        let r = row as i32 + dr;
                        let c = col as i32 + dc;
                        if r < 0 || r >= height as i32 || c >= width as i32 {
                            continue;
                        }
                        if let Some(there) = grid[r as usize][c as usize] {
                            link(&mut lanes, here, there);
                            if dr == 0 {
                                break;
                            }
                        }
                    }
                }
                if row + 1 < height {
                    if let Some(there) = grid[row + 1][col] {
                        link(&mut lanes, here, there);
                    }
                }
            }
        }
        // A handful of hyperlanes, so the deep is not always a long haul.
        for _ in 0..width / 2 {
            let a = (rand() % nodes.len() as u64) as usize;
            let b = (rand() % nodes.len() as u64) as usize;
            let far = nodes[a].pos.0.abs_diff(nodes[b].pos.0);
            if (3..=6).contains(&far) {
                link(&mut lanes, a, b);
            }
        }
        let start = grid[height / 2][0]
            .or_else(|| (0..height).find_map(|r| grid[r][0]))
            .unwrap_or(0);
        let mut map = StarMap {
            galaxy,
            nodes,
            grid,
            lanes,
            at: start,
            cursor: start,
            width,
            height,
        };
        map.nodes[start].cleared = true;
        map.chart_surroundings();
        map.cursor = map.reachable().first().copied().unwrap_or(start);
        map
    }

    /// Everything one lane out of the current system is on the chart now.
    fn chart_surroundings(&mut self) {
        self.nodes[self.at].explored = true;
        for i in self.lanes[self.at].clone() {
            self.nodes[i].explored = true;
        }
    }

    /// The systems one lane away from where the squad is parked.
    pub fn reachable(&self) -> Vec<usize> {
        self.reachable_within(1)
    }

    /// Everything within `range` lanes, which is what a tuned hyperdrive buys.
    pub fn reachable_within(&self, range: usize) -> Vec<usize> {
        let mut seen = vec![false; self.nodes.len()];
        let mut edge = vec![self.at];
        seen[self.at] = true;
        let mut out = Vec::new();
        for _ in 0..range.max(1) {
            let mut next = Vec::new();
            for &node in &edge {
                for &lane in &self.lanes[node] {
                    if !seen[lane] {
                        seen[lane] = true;
                        out.push(lane);
                        next.push(lane);
                    }
                }
            }
            edge = next;
        }
        out.sort_by_key(|&i| (self.nodes[i].pos.0, self.nodes[i].pos.1));
        out
    }

    /// Fly a lane, or a run of them, to the system under the cursor.
    pub fn jump_within(&mut self, range: usize) -> Option<MapNode> {
        if !self.reachable_within(range).contains(&self.cursor) {
            return None;
        }
        self.at = self.cursor;
        self.chart_surroundings();
        let node = self.nodes[self.at];
        self.cursor = self.reachable().first().copied().unwrap_or(self.at);
        Some(node)
    }

    /// Step the chart cursor through the reachable systems.
    pub fn move_cursor(&mut self, delta: i32) {
        let lanes = self.reachable();
        if lanes.is_empty() {
            return;
        }
        let at = lanes.iter().position(|&n| n == self.cursor).unwrap_or(0) as i32;
        let next = (at + delta).rem_euclid(lanes.len() as i32) as usize;
        self.cursor = lanes[next];
    }

    /// Point the cursor at whichever reachable system lies that way.
    pub fn steer(&mut self, dc: i32, dr: i32) {
        let here = self.nodes[self.at].pos;
        let best = self
            .reachable()
            .into_iter()
            .filter(|&i| {
                let there = self.nodes[i].pos;
                let (dx, dy) = (
                    there.0 as i32 - here.0 as i32,
                    there.1 as i32 - here.1 as i32,
                );
                (dc != 0 && dx.signum() == dc) || (dr != 0 && dy.signum() == dr)
            })
            .min_by_key(|&i| {
                let there = self.nodes[i].pos;
                (there.0 as i32 - here.0 as i32).abs() + (there.1 as i32 - here.1 as i32).abs()
            });
        if let Some(next) = best {
            self.cursor = next;
        }
    }

    /// Fly to the system under the cursor, if a lane runs to it.
    pub fn jump(&mut self) -> Option<MapNode> {
        if !self.lanes[self.at].contains(&self.cursor) {
            return None;
        }
        self.at = self.cursor;
        self.chart_surroundings();
        let node = self.nodes[self.at];
        self.cursor = self.reachable().first().copied().unwrap_or(self.at);
        Some(node)
    }

    /// Mark the system the squad is parked at as done.
    pub fn clear_here(&mut self) {
        self.nodes[self.at].cleared = true;
    }

    pub fn here(&self) -> MapNode {
        self.nodes[self.at]
    }

    /// How much of the galaxy has been seen.
    pub fn charted(&self) -> usize {
        self.nodes.iter().filter(|n| n.explored).count()
    }

    /// How much of it has been cleared out.
    pub fn cleared(&self) -> usize {
        self.nodes.iter().filter(|n| n.cleared).count()
    }
}

/// The class of capital ship holding a system.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapitalKind {
    /// A picket: short, lightly gunned, one dome.
    ImperialFrigate,
    /// The wedge: a long arrowhead bristling with batteries, its domes on the
    /// tower and its bays feeding fighters into the court.
    StarDestroyer,
    /// A moon-sized plate of armour with a trench cut across it, and a port at
    /// the bottom of the trench that ends it.
    DeathStar,
    /// The command ship: a wedge twice the size, and twice the batteries.
    SuperDestroyer,
    /// An interdictor: gravity-well projectors that hold a hull in the system
    /// and drag it wherever they please.
    Interdictor,
    /// A Mon Calamari cruiser: the Alliance's own capital, which fights on your
    /// side and can be lost.
    MonCalamari,
}

impl CapitalKind {
    pub fn name(self) -> &'static str {
        match self {
            CapitalKind::ImperialFrigate => "Imperial frigate",
            CapitalKind::StarDestroyer => "Star Destroyer",
            CapitalKind::DeathStar => "Death Star",
            CapitalKind::SuperDestroyer => "Super Star Destroyer",
            CapitalKind::Interdictor => "Interdictor cruiser",
            CapitalKind::MonCalamari => "Mon Calamari cruiser",
        }
    }

    /// Rows of hull, counted down from the anchor.
    pub fn depth(self) -> i16 {
        match self {
            CapitalKind::ImperialFrigate => 3,
            CapitalKind::StarDestroyer => 6,
            CapitalKind::DeathStar => 8,
            CapitalKind::SuperDestroyer => 9,
            CapitalKind::Interdictor => 5,
            CapitalKind::MonCalamari => 4,
        }
    }

    /// Half-width of the hull on a given row of it: the wedge tapers, the
    /// station is a wall.
    pub fn span(self, row: i16) -> i16 {
        match self {
            CapitalKind::ImperialFrigate => 9,
            CapitalKind::StarDestroyer => 4 + row * 3,
            CapitalKind::DeathStar => W / 2 - 2,
            CapitalKind::SuperDestroyer => 5 + row * 4,
            CapitalKind::Interdictor => 10,
            CapitalKind::MonCalamari => 12,
        }
    }

    /// Hull points before the campaign scales it.
    pub fn hull(self) -> i32 {
        match self {
            CapitalKind::ImperialFrigate => 120,
            CapitalKind::StarDestroyer => 260,
            CapitalKind::DeathStar => 400,
            CapitalKind::SuperDestroyer => 700,
            CapitalKind::Interdictor => 220,
            CapitalKind::MonCalamari => 320,
        }
    }
}

/// What a piece of a capital ship does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Emplacement {
    /// While one still stands the hull cannot be touched at all.
    ShieldDome,
    /// Heavy batteries: two shield pips a hit.
    Turbolaser,
    /// Launches fighters until it is knocked out.
    HangarBay,
    /// The command tower: without it the ship's guns are half as quick.
    CommandTower,
    /// The engines: without them the ship cannot hold station.
    EngineBank,
    /// Drags the hull toward the ship.
    TractorBeam,
    /// The weak point at the bottom of the trench. One hit through the shields
    /// takes the whole ship.
    ExhaustPort,
    /// A gravity-well projector: it drags harder than a tractor beam, and while
    /// one is up no hull in the system can go to lightspeed.
    GravityProjector,
}

impl Emplacement {
    pub fn name(self) -> &'static str {
        match self {
            Emplacement::ShieldDome => "shield generator",
            Emplacement::Turbolaser => "turbolaser battery",
            Emplacement::HangarBay => "hangar bay",
            Emplacement::CommandTower => "bridge tower",
            Emplacement::EngineBank => "ion engines",
            Emplacement::TractorBeam => "tractor beam",
            Emplacement::ExhaustPort => "thermal exhaust port",
            Emplacement::GravityProjector => "gravity-well projector",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Emplacement::ShieldDome => "◓",
            Emplacement::Turbolaser => "╪",
            Emplacement::HangarBay => "⊓",
            Emplacement::CommandTower => "♜",
            Emplacement::EngineBank => "◙",
            Emplacement::TractorBeam => "◈",
            Emplacement::ExhaustPort => "◎",
            Emplacement::GravityProjector => "◍",
        }
    }

    pub fn hp(self) -> i32 {
        match self {
            Emplacement::ShieldDome => 26,
            Emplacement::Turbolaser => 14,
            Emplacement::HangarBay => 20,
            Emplacement::CommandTower => 30,
            Emplacement::EngineBank => 18,
            Emplacement::TractorBeam => 16,
            Emplacement::ExhaustPort => 6,
            Emplacement::GravityProjector => 22,
        }
    }

    pub fn score(self) -> u32 {
        match self {
            Emplacement::ShieldDome => 400,
            Emplacement::Turbolaser => 200,
            Emplacement::HangarBay => 300,
            Emplacement::CommandTower => 500,
            Emplacement::EngineBank => 250,
            Emplacement::TractorBeam => 350,
            Emplacement::ExhaustPort => 1_000,
            Emplacement::GravityProjector => 450,
        }
    }
}

/// One emplacement bolted to a capital hull.
#[derive(Clone, Debug)]
pub struct CapitalPart {
    pub kind: Emplacement,
    /// Offset from the ship's anchor, as `(row, column)`.
    pub offset: (i16, i16),
    pub hp: i32,
    pub max_hp: i32,
    cooldown: u32,
    /// Ticks this emplacement is still ion-scrambled for.
    pub ion: u32,
}

impl CapitalPart {
    fn new(kind: Emplacement, offset: (i16, i16), armour: i32) -> CapitalPart {
        let hp = kind.hp() + armour * 4;
        CapitalPart {
            kind,
            offset,
            hp,
            max_hp: hp,
            cooldown: kind.hp() as u32 % 17 + 8,
            ion: 0,
        }
    }

    /// A scrambled emplacement does nothing at all until it comes back.
    pub fn live(&self) -> bool {
        self.ion == 0
    }
}

/// A capital ship holding a system: a wall of hull, its emplacements, and the
/// order they have to come apart in.
#[derive(Clone, Debug)]
pub struct Capital {
    pub kind: CapitalKind,
    /// Anchor: the top row of the hull and the column it is centred on.
    pub pos: (i16, i16),
    pub hp: i32,
    pub max_hp: i32,
    pub dir: i16,
    pub parts: Vec<CapitalPart>,
    tick: u32,
}

impl Capital {
    pub fn new(kind: CapitalKind, armour: i32, hull_bonus: i32) -> Capital {
        let hp = kind.hull() + hull_bonus;
        let mut parts = Vec::new();
        let mut fit = |kind, offset| parts.push(CapitalPart::new(kind, offset, armour));
        match kind {
            CapitalKind::ImperialFrigate => {
                fit(Emplacement::ShieldDome, (1, 0));
                fit(Emplacement::Turbolaser, (2, -6));
                fit(Emplacement::Turbolaser, (2, 6));
                fit(Emplacement::HangarBay, (2, 0));
                fit(Emplacement::EngineBank, (0, -8));
            }
            CapitalKind::StarDestroyer => {
                fit(Emplacement::CommandTower, (1, 0));
                fit(Emplacement::ShieldDome, (1, -3));
                fit(Emplacement::ShieldDome, (1, 3));
                fit(Emplacement::Turbolaser, (3, -9));
                fit(Emplacement::Turbolaser, (3, 9));
                fit(Emplacement::Turbolaser, (5, -14));
                fit(Emplacement::Turbolaser, (5, 14));
                fit(Emplacement::HangarBay, (5, -5));
                fit(Emplacement::HangarBay, (5, 5));
                fit(Emplacement::TractorBeam, (4, 0));
                fit(Emplacement::EngineBank, (0, -6));
                fit(Emplacement::EngineBank, (0, 6));
            }
            CapitalKind::MonCalamari => {
                fit(Emplacement::ShieldDome, (1, -6));
                fit(Emplacement::ShieldDome, (1, 6));
                fit(Emplacement::Turbolaser, (0, -9));
                fit(Emplacement::Turbolaser, (0, 0));
                fit(Emplacement::Turbolaser, (0, 9));
                fit(Emplacement::HangarBay, (3, 0));
                fit(Emplacement::EngineBank, (3, -9));
            }
            CapitalKind::Interdictor => {
                fit(Emplacement::ShieldDome, (1, 0));
                fit(Emplacement::GravityProjector, (2, -7));
                fit(Emplacement::GravityProjector, (2, 7));
                fit(Emplacement::Turbolaser, (3, -4));
                fit(Emplacement::Turbolaser, (3, 4));
                fit(Emplacement::HangarBay, (4, 0));
                fit(Emplacement::EngineBank, (0, 0));
            }
            CapitalKind::SuperDestroyer => {
                fit(Emplacement::CommandTower, (1, 0));
                fit(Emplacement::ShieldDome, (1, -4));
                fit(Emplacement::ShieldDome, (1, 4));
                fit(Emplacement::ShieldDome, (2, 0));
                for dx in [-12, -6, 6, 12] {
                    fit(Emplacement::Turbolaser, (4, dx));
                }
                for dx in [-20, -10, 10, 20] {
                    fit(Emplacement::Turbolaser, (7, dx));
                }
                fit(Emplacement::HangarBay, (8, -8));
                fit(Emplacement::HangarBay, (8, 0));
                fit(Emplacement::HangarBay, (8, 8));
                fit(Emplacement::TractorBeam, (6, -4));
                fit(Emplacement::TractorBeam, (6, 4));
                for dx in [-8, 0, 8] {
                    fit(Emplacement::EngineBank, (0, dx));
                }
            }
            CapitalKind::DeathStar => {
                fit(Emplacement::ShieldDome, (1, -18));
                fit(Emplacement::ShieldDome, (1, 18));
                fit(Emplacement::ShieldDome, (2, -8));
                fit(Emplacement::ShieldDome, (2, 8));
                fit(Emplacement::Turbolaser, (4, -22));
                fit(Emplacement::Turbolaser, (4, -13));
                fit(Emplacement::Turbolaser, (4, 13));
                fit(Emplacement::Turbolaser, (4, 22));
                fit(Emplacement::HangarBay, (6, -17));
                fit(Emplacement::HangarBay, (6, 17));
                fit(Emplacement::TractorBeam, (6, 0));
                // The port sits at the bottom of the trench, dead centre.
                fit(Emplacement::ExhaustPort, (7, 0));
            }
        }
        Capital {
            kind,
            pos: (0, W / 2),
            hp,
            max_hp: hp,
            dir: 1,
            parts,
            tick: 0,
        }
    }

    /// Emplacements of a kind that are still standing.
    pub fn standing(&self, kind: Emplacement) -> usize {
        self.parts
            .iter()
            .filter(|p| p.kind == kind && p.hp > 0)
            .count()
    }

    /// While a dome is up and unscrambled, nothing reaches the hull.
    pub fn shielded(&self) -> bool {
        self.parts
            .iter()
            .any(|p| p.kind == Emplacement::ShieldDome && p.hp > 0 && p.live())
    }

    /// The absolute cell an emplacement sits in.
    pub fn part_cell(&self, part: &CapitalPart) -> (i16, i16) {
        (self.pos.0 + part.offset.0, self.pos.1 + part.offset.1)
    }

    /// Is this cell hull plating?
    pub fn covers(&self, row: i16, col: i16) -> bool {
        let dr = row - self.pos.0;
        if !(0..self.kind.depth()).contains(&dr) {
            return false;
        }
        (col - self.pos.1).abs() <= self.kind.span(dr)
    }

    /// Ticks between volleys; losing the tower slows the whole ship down.
    fn cadence(&self) -> u32 {
        let tower = self.standing(Emplacement::CommandTower) > 0
            || !self
                .parts
                .iter()
                .any(|p| p.kind == Emplacement::CommandTower);
        let base = match self.kind {
            CapitalKind::ImperialFrigate => 20,
            CapitalKind::StarDestroyer => 16,
            CapitalKind::DeathStar => 12,
            CapitalKind::SuperDestroyer => 10,
            CapitalKind::Interdictor => 18,
            CapitalKind::MonCalamari => 14,
        };
        if tower {
            base
        } else {
            base * 2
        }
    }

    /// It only holds station while an engine bank is left.
    fn under_way(&self) -> bool {
        self.standing(Emplacement::EngineBank) > 0
    }
}

/// What a pilot can do with the Force, once there is enough of it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ForcePower {
    /// Stretch out: everything the Empire throws flies at half speed.
    Sense,
    /// Pull: every loose pickup on the court comes to the hull.
    Pull,
    /// Let go: the next torpedo salvo flies itself into a weak point.
    Guided,
}

impl ForcePower {
    pub fn name(self) -> &'static str {
        match self {
            ForcePower::Sense => "sense",
            ForcePower::Pull => "pull",
            ForcePower::Guided => "guided",
        }
    }

    pub fn cost(self) -> u32 {
        match self {
            ForcePower::Sense => SENSE_COST,
            ForcePower::Pull => PULL_COST,
            ForcePower::Guided => GUIDED_COST,
        }
    }
}

/// What the Alliance calls you, by the flying you have done.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Rank {
    FlightCadet,
    FlightOfficer,
    Lieutenant,
    Captain,
    Commander,
    General,
}

impl Rank {
    /// The rank a pilot of this level has earned.
    pub fn of_level(level: u32) -> Rank {
        match level {
            0..=2 => Rank::FlightCadet,
            3..=5 => Rank::FlightOfficer,
            6..=9 => Rank::Lieutenant,
            10..=14 => Rank::Captain,
            15..=21 => Rank::Commander,
            _ => Rank::General,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Rank::FlightCadet => "Flight Cadet",
            Rank::FlightOfficer => "Flight Officer",
            Rank::Lieutenant => "Lieutenant",
            Rank::Captain => "Captain",
            Rank::Commander => "Commander",
            Rank::General => "General",
        }
    }
}

/// One line of squadron radio traffic, and how long it stays up.
#[derive(Clone, Debug)]
pub struct Chatter {
    pub line: String,
    pub ticks: u32,
}

/// A place to stand in the hangar: a bay with a fighter in it, or a station
/// somebody works at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeckSpot {
    /// A parked fighter, by its place in the squadron.
    Bay(usize),
    /// The quartermaster's terminal: everything the yard sells.
    Quartermaster,
    /// The navicomputer: the galaxy chart, and the way out.
    Navicomputer,
    /// The astromech pit: droids patch the squadron up here.
    AstromechPit,
    /// The briefing table: what the next mission wants.
    BriefingTable,
    /// The launch pad: walk onto it to fly.
    LaunchPad,
    /// Your own fighter, parked on the surface: walk to it to lift off.
    ParkedShip,
    /// A cantina: drinks, rumours, and somebody who knows where the Empire is.
    Cantina,
    /// A wreck worth stripping.
    SurfaceWreck,
    /// A settlement that trades.
    Settlement,
    /// An outpost with a droid and a power cell.
    Outpost,
    /// Ruins older than the war.
    Ruins,
}

impl DeckSpot {
    pub fn name(self) -> &'static str {
        match self {
            DeckSpot::Bay(_) => "fighter bay",
            DeckSpot::Quartermaster => "quartermaster",
            DeckSpot::Navicomputer => "navicomputer",
            DeckSpot::AstromechPit => "astromech pit",
            DeckSpot::BriefingTable => "briefing table",
            DeckSpot::LaunchPad => "launch pad",
            DeckSpot::ParkedShip => "your fighter",
            DeckSpot::Cantina => "cantina",
            DeckSpot::SurfaceWreck => "wreck",
            DeckSpot::Settlement => "settlement",
            DeckSpot::Outpost => "outpost",
            DeckSpot::Ruins => "ruins",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            DeckSpot::Bay(_) => "▣",
            DeckSpot::Quartermaster => "⌸",
            DeckSpot::Navicomputer => "⌺",
            DeckSpot::AstromechPit => "◍",
            DeckSpot::BriefingTable => "⌹",
            DeckSpot::LaunchPad => "◎",
            DeckSpot::ParkedShip => "✕",
            DeckSpot::Cantina => "⌂",
            DeckSpot::SurfaceWreck => "⌗",
            DeckSpot::Settlement => "⌸",
            DeckSpot::Outpost => "⌺",
            DeckSpot::Ruins => "⍟",
        }
    }
}

/// Everything a pilot can carry on his belt: the blasters, and the blades.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SideArm {
    /// The standard-issue sidearm: nothing special, always works.
    ServicePistol,
    /// A heavy pistol: slow, and it drops a trooper in one.
    HeavyPistol,
    /// The trooper carbine: quick, and not fussy about where it points.
    Carbine,
    /// A hold-out blaster: fast, weak, and it fits anywhere.
    HoldOut,
    /// A scattergun: a spread of pellets, useless past a corridor.
    Scattergun,
    /// A bowcaster: one heavy quarrel that goes through what it hits.
    Bowcaster,
    /// A disruptor: slow, illegal, and nothing survives it.
    Disruptor,
    /// A long rifle: the reach of a rifle and the patience to use it.
    LongRifle,
    /// A repeater: it hoses, and it is never quite on target.
    Repeater,
    /// A single blade.
    Sabre,
    /// A shorter blade in the off hand as well: two cuts, less reach.
    DualSabres,
    /// A staff lit at both ends: it comes round further than you expect.
    SabreStaff,
    /// A crossguard blade: heavy, and it holds a bind.
    CrossguardSabre,
}

impl SideArm {
    pub const ALL: [SideArm; 13] = [
        SideArm::ServicePistol,
        SideArm::HeavyPistol,
        SideArm::Carbine,
        SideArm::HoldOut,
        SideArm::Scattergun,
        SideArm::Bowcaster,
        SideArm::Disruptor,
        SideArm::LongRifle,
        SideArm::Repeater,
        SideArm::Sabre,
        SideArm::DualSabres,
        SideArm::SabreStaff,
        SideArm::CrossguardSabre,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SideArm::ServicePistol => "service pistol",
            SideArm::HeavyPistol => "heavy pistol",
            SideArm::Carbine => "carbine",
            SideArm::HoldOut => "hold-out blaster",
            SideArm::Scattergun => "scattergun",
            SideArm::Bowcaster => "bowcaster",
            SideArm::Disruptor => "disruptor",
            SideArm::LongRifle => "long rifle",
            SideArm::Repeater => "repeater",
            SideArm::Sabre => "lightsabre",
            SideArm::DualSabres => "dual sabres",
            SideArm::SabreStaff => "sabre staff",
            SideArm::CrossguardSabre => "crossguard sabre",
        }
    }

    /// A blade rather than a barrel.
    pub fn is_blade(self) -> bool {
        matches!(
            self,
            SideArm::Sabre | SideArm::DualSabres | SideArm::SabreStaff | SideArm::CrossguardSabre
        )
    }

    /// What one hit takes off.
    pub fn damage(self) -> i32 {
        match self {
            SideArm::ServicePistol => 2,
            SideArm::HeavyPistol => 4,
            SideArm::Carbine => 2,
            SideArm::HoldOut => 1,
            SideArm::Scattergun => 2,
            SideArm::Bowcaster => 5,
            SideArm::Disruptor => 9,
            SideArm::LongRifle => 6,
            SideArm::Repeater => 1,
            SideArm::Sabre => 6,
            SideArm::DualSabres => 4,
            SideArm::SabreStaff => 5,
            SideArm::CrossguardSabre => 8,
        }
    }

    /// Ticks between shots or cuts.
    pub fn cadence(self) -> u32 {
        match self {
            SideArm::ServicePistol => 3,
            SideArm::HeavyPistol => 7,
            SideArm::Carbine => 3,
            SideArm::HoldOut => 2,
            SideArm::Scattergun => 8,
            SideArm::Bowcaster => 9,
            SideArm::Disruptor => 14,
            SideArm::LongRifle => 11,
            SideArm::Repeater => 1,
            SideArm::Sabre => 4,
            SideArm::DualSabres => 3,
            SideArm::SabreStaff => 5,
            SideArm::CrossguardSabre => 6,
        }
    }

    /// How far a bolt carries, or how far a blade reaches.
    pub fn range(self) -> u32 {
        match self {
            SideArm::HoldOut => 8,
            SideArm::Scattergun => 7,
            SideArm::Carbine | SideArm::ServicePistol | SideArm::Repeater => 18,
            SideArm::HeavyPistol | SideArm::Bowcaster => 20,
            SideArm::Disruptor => 22,
            SideArm::LongRifle => 30,
            _ => 2,
        }
    }

    /// Bolts per shot: a scattergun throws a handful, a repeater one at a time.
    pub fn pellets(self) -> i16 {
        match self {
            SideArm::Scattergun => 3,
            SideArm::DualSabres => 2,
            _ => 1,
        }
    }

    /// How far to the side a blade comes round.
    pub fn arc(self) -> i16 {
        match self {
            SideArm::SabreStaff => 2,
            SideArm::DualSabres => 1,
            SideArm::CrossguardSabre => 1,
            SideArm::Sabre => 1,
            _ => 0,
        }
    }

    /// What the quartermaster wants for one.
    pub fn price(self) -> u32 {
        match self {
            SideArm::ServicePistol => 150,
            SideArm::HoldOut => 200,
            SideArm::Carbine => 400,
            SideArm::Scattergun => 550,
            SideArm::HeavyPistol => 700,
            SideArm::Repeater => 900,
            SideArm::Bowcaster => 1_100,
            SideArm::LongRifle => 1_400,
            SideArm::Disruptor => 2_200,
            SideArm::Sabre => 3_000,
            SideArm::DualSabres => 3_600,
            SideArm::SabreStaff => 4_200,
            SideArm::CrossguardSabre => 4_800,
        }
    }

    /// What he will give you back for one, which is never what you paid.
    pub fn resale(self) -> u32 {
        self.price() / 2
    }
}

/// What kind of thing is walking about down there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GroundKind {
    /// A stormtrooper patrol.
    Trooper,
    /// A scout walker: two legs, a chin gun and far too much armour for a
    /// sidearm.
    Scout,
    /// A duellist who has been waiting for you, and does not use a blaster.
    Duellist,
}

impl GroundKind {
    pub fn name(self) -> &'static str {
        match self {
            GroundKind::Trooper => "stormtrooper",
            GroundKind::Scout => "scout walker",
            GroundKind::Duellist => "duellist",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            GroundKind::Trooper => "Ω",
            GroundKind::Scout => "Ѫ",
            GroundKind::Duellist => "Ѱ",
        }
    }

    pub fn hp(self) -> i32 {
        match self {
            GroundKind::Trooper => 3,
            GroundKind::Scout => 14,
            GroundKind::Duellist => 24,
        }
    }

    /// How hard it hits, and how close it has to be to do it.
    pub fn damage(self) -> i32 {
        match self {
            GroundKind::Trooper => 1,
            GroundKind::Scout => 2,
            GroundKind::Duellist => 3,
        }
    }

    pub fn reach(self) -> i16 {
        match self {
            GroundKind::Duellist => 2,
            _ => 14,
        }
    }
}

/// A stormtrooper patrol, or whatever the world has walking about on it.
#[derive(Clone, Debug)]
pub struct Trooper {
    pub kind: GroundKind,
    pub pos: (i16, i16),
    pub hp: i32,
    pub cooldown: u32,
}

impl Trooper {
    fn new(kind: GroundKind, pos: (i16, i16), cooldown: u32) -> Trooper {
        Trooper {
            kind,
            pos,
            hp: kind.hp(),
            cooldown,
        }
    }
}

/// How a duellist is fighting you at the moment.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stance {
    /// Circling: he is looking for the opening rather than making one.
    Measured,
    /// Pressing: shorter wind-ups, harder cuts.
    Aggressive,
    /// Guarded: he is waiting for you to come to him, and he will parry it.
    Defensive,
}

impl Stance {
    pub fn name(self) -> &'static str {
        match self {
            Stance::Measured => "measured",
            Stance::Aggressive => "aggressive",
            Stance::Defensive => "guarded",
        }
    }

    /// Ticks he spends winding up, which is your window to do something.
    fn wind_up(self) -> u32 {
        match self {
            Stance::Measured => 14,
            Stance::Aggressive => 9,
            Stance::Defensive => 20,
        }
    }

    /// What his cut takes off you if it lands clean.
    fn damage(self) -> i32 {
        match self {
            Stance::Aggressive => 4,
            _ => 3,
        }
    }
}

/// A sabre duel: two blades, two guards, and whoever loses their footing.
#[derive(Clone, Debug)]
pub struct Duel {
    /// Which of the things on the ground you are locked up with.
    pub foe: usize,
    pub stance: Stance,
    /// Ticks until his cut lands. Zero means he is not swinging.
    pub wind_up: u32,
    /// Ticks your guard is still up for.
    pub guard: u32,
    /// Ticks your riposte window is open for after a clean parry.
    pub riposte: u32,
    /// How much footing each side has left; at nothing you are wide open.
    pub poise: i32,
    pub foe_poise: i32,
    /// Ticks of blade lock, and who is winning it.
    pub lock: u32,
    pub pressure: i32,
    /// Ticks either side is staggered and cannot do anything about it.
    pub stagger: u32,
    pub foe_stagger: u32,
}

impl Duel {
    fn new(foe: usize, stance: Stance) -> Duel {
        Duel {
            foe,
            stance,
            wind_up: 0,
            guard: 0,
            riposte: 0,
            poise: DUEL_POISE,
            foe_poise: DUEL_POISE,
            lock: 0,
            pressure: 0,
            stagger: 0,
            foe_stagger: 0,
        }
    }

    /// What the readout should be telling you to do right now.
    pub fn prompt(&self) -> &'static str {
        if self.lock > 0 {
            "BLADE LOCK — hit it again"
        } else if self.stagger > 0 {
            "OFF BALANCE"
        } else if self.foe_stagger > 0 {
            "HE IS OPEN — cut him"
        } else if self.riposte > 0 {
            "RIPOSTE"
        } else if self.wind_up > 0 && self.wind_up <= PARRY_WINDOW {
            "HE SWINGS — parry"
        } else if self.wind_up > 0 {
            "he winds up"
        } else {
            "circling"
        }
    }
}

/// A blaster bolt on foot, from either side.
#[derive(Clone, Debug)]
pub struct Bolt {
    pub pos: (i16, i16),
    pub dir: (i16, i16),
    pub friendly: bool,
    pub life: u32,
}

/// A block of city, a rock or a stand of trees: something to take cover behind.
#[derive(Clone, Copy, Debug)]
pub struct Cover {
    pub pos: (i16, i16),
    pub size: (i16, i16),
    pub tall: bool,
}

/// What walking into something on the deck did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeckAction {
    Boarded(usize),
    LiftedOff,
    Drank,
    Stripped,
    Traded,
    Resupplied,
    Explored,
    OpenedShop,
    OpenedChart,
    Repaired,
    ReadBriefing,
    Launched,
}

/// The hangar deck itself: a floor to walk about on, fighters parked along the
/// back wall and the stations the squadron uses down the front.
#[derive(Clone, Debug)]
pub struct Deck {
    pub width: i16,
    pub height: i16,
    /// Where the pilot is standing, as `(row, column)`.
    pub pilot: (i16, i16),
    /// Which way they are facing, for the first-person view.
    pub facing: (i16, i16),
    /// Everything worth walking up to.
    pub spots: Vec<(DeckSpot, (i16, i16))>,
    /// Blocks, rocks and trees: solid, and worth hiding behind.
    pub cover: Vec<Cover>,
    /// Imperial patrols on the ground.
    pub troopers: Vec<Trooper>,
    /// Blaster fire on foot.
    pub bolts: Vec<Bolt>,
    /// What the pilot can take before he goes down.
    pub health: i32,
    pub blaster_cooldown: u32,
    /// What is in his hands, and whether he is on a speeder.
    pub sidearm: SideArm,
    pub riding: bool,
    /// The duel he is in the middle of, if a blade has found him.
    pub duel: Option<Duel>,
}

impl Deck {
    pub const WIDTH: i16 = 140;
    pub const HEIGHT: i16 = 44;
    /// How close counts as standing at something, and how far ahead counts as
    /// being in front of you.
    pub const REACH: i16 = 2;
    pub const SIGHT: i16 = 5;

    /// Lay out a stretch of surface on `planet`, with the fighter parked on it
    /// and whatever the world has to walk to.
    pub fn surface(planet: Planet, seed: u64) -> Deck {
        let mut rng = seed | 1;
        let mut rand = move || {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            rng >> 33
        };
        let mut spots = vec![(DeckSpot::ParkedShip, (Deck::HEIGHT - 4, Deck::WIDTH / 2))];
        let stock: &[DeckSpot] = match planet {
            Planet::Tatooine | Planet::Jakku | Planet::Mandalore => &[
                DeckSpot::Cantina,
                DeckSpot::SurfaceWreck,
                DeckSpot::Settlement,
            ],
            Planet::Hoth | Planet::Dagobah | Planet::Mustafar => {
                &[DeckSpot::Outpost, DeckSpot::SurfaceWreck, DeckSpot::Ruins]
            }
            Planet::Coruscant | Planet::Naboo | Planet::Bespin => {
                &[DeckSpot::Settlement, DeckSpot::Cantina, DeckSpot::Outpost]
            }
            _ => &[
                DeckSpot::Outpost,
                DeckSpot::Ruins,
                DeckSpot::SurfaceWreck,
                DeckSpot::Cantina,
            ],
        };
        for (i, &spot) in stock.iter().enumerate() {
            let row = Deck::HEIGHT - 16 + (rand() % 10) as i16;
            let col = Deck::WIDTH / 2 - 24 + i as i16 * 16 + (rand() % 5) as i16;
            spots.push((spot, (row, col.min(Deck::WIDTH - 4))));
        }
        // A world is not an empty floor: blocks of city, rock or trees, and
        // whoever the Empire has walking about.
        let mut cover = Vec::new();
        let city = matches!(
            planet,
            Planet::Coruscant | Planet::Bespin | Planet::Naboo | Planet::Mandalore
        );
        let blocks = if city { 26 } else { 18 };
        for _ in 0..blocks {
            let row = 2 + (rand() % (Deck::HEIGHT as u64 - 6)) as i16;
            let col = 3 + (rand() % (Deck::WIDTH as u64 - 8)) as i16;
            let size = if city {
                (2 + (rand() % 4) as i16, 4 + (rand() % 8) as i16)
            } else {
                (1 + (rand() % 2) as i16, 1 + (rand() % 3) as i16)
            };
            cover.push(Cover {
                pos: (row, col),
                size,
                tall: city,
            });
        }
        let mut troopers = Vec::new();
        for _ in 0..(6 + (rand() % 6) as usize) {
            let row = 2 + (rand() % (Deck::HEIGHT as u64 - 6)) as i16;
            let col = 3 + (rand() % (Deck::WIDTH as u64 - 6)) as i16;
            // Mostly troopers, the odd scout walker, and on the old worlds
            // somebody waiting with a sabre.
            let kind = match rand() % 10 {
                0 | 1 => GroundKind::Scout,
                2 if matches!(planet, Planet::Dagobah | Planet::Mustafar | Planet::Endor) => {
                    GroundKind::Duellist
                }
                _ => GroundKind::Trooper,
            };
            troopers.push(Trooper::new(kind, (row, col), (rand() % 30) as u32));
        }
        Deck {
            width: Deck::WIDTH,
            height: Deck::HEIGHT,
            pilot: (Deck::HEIGHT - 9, Deck::WIDTH / 2),
            facing: (-1, 0),
            spots,
            cover,
            troopers,
            bolts: Vec::new(),
            health: PILOT_HEALTH,
            blaster_cooldown: 0,
            sidearm: SideArm::ServicePistol,
            riding: false,
            duel: None,
        }
    }

    /// Lay the deck out for a squadron of `hulls` fighters.
    pub fn new(hulls: usize) -> Deck {
        let mut spots = Vec::new();
        for i in 0..hulls.max(1) {
            // Two ranks of bays along the back wall, so a full squadron fits.
            let rank = i / 3;
            let col = 12 + (i % 3) as i16 * 16;
            let row = 4 + rank as i16 * 6;
            spots.push((DeckSpot::Bay(i), (row, col.min(Deck::WIDTH - 6))));
        }
        spots.push((DeckSpot::Quartermaster, (18, 8)));
        spots.push((DeckSpot::AstromechPit, (18, 20)));
        spots.push((DeckSpot::BriefingTable, (18, 32)));
        spots.push((DeckSpot::Navicomputer, (18, 44)));
        spots.push((DeckSpot::LaunchPad, (22, 24)));
        Deck {
            width: Deck::WIDTH,
            height: Deck::HEIGHT,
            pilot: (12, 24),
            facing: (-1, 0),
            spots,
            cover: Vec::new(),
            troopers: Vec::new(),
            bolts: Vec::new(),
            health: PILOT_HEALTH,
            blaster_cooldown: 0,
            sidearm: SideArm::ServicePistol,
            riding: false,
            duel: None,
        }
    }

    /// Is this cell inside something solid?
    pub fn blocked(&self, row: i16, col: i16) -> bool {
        self.cover.iter().any(|block| {
            (block.pos.0..block.pos.0 + block.size.0).contains(&row)
                && (block.pos.1..block.pos.1 + block.size.1).contains(&col)
        })
    }

    /// Turn on the spot: `-1` to port, `1` to starboard.
    pub fn turn(&mut self, dir: i16) {
        // The four ways to face, in order, so a turn is one step round them.
        const WAYS: [(i16, i16); 4] = [(-1, 0), (0, 1), (1, 0), (0, -1)];
        let at = WAYS.iter().position(|&way| way == self.facing).unwrap_or(0) as i16;
        let next = (at + dir).rem_euclid(WAYS.len() as i16) as usize;
        self.facing = WAYS[next];
    }

    /// Walk the way you are facing, or back off it.
    pub fn step(&mut self, ahead: i16) {
        let (dr, dc) = self.facing;
        // A speeder covers ground three times as fast, and a tree at that speed
        // is the end of the ride.
        let pace = if self.riding { SPEEDER_PACE } else { 1 };
        for _ in 0..pace {
            let row = (self.pilot.0 + dr * ahead).clamp(1, self.height - 2);
            let col = (self.pilot.1 + dc * ahead).clamp(1, self.width - 2);
            if self.blocked(row, col) {
                if self.riding {
                    self.riding = false;
                    self.health -= SPEEDER_CRASH;
                }
                return;
            }
            self.pilot = (row, col);
        }
    }

    /// Use whatever is in the pilot's hands: a bolt down the lane, or a cut
    /// through whatever is standing in front of him.
    pub fn shoot(&mut self) -> bool {
        if self.blaster_cooldown > 0 || self.riding {
            return false;
        }
        let (dr, dc) = self.facing;
        let arm = self.sidearm;
        self.blaster_cooldown = arm.cadence();
        match arm {
            barrel if !barrel.is_blade() => {
                // A scattergun throws a handful; everything else one bolt.
                for pellet in 0..barrel.pellets() {
                    let spread = pellet - barrel.pellets() / 2;
                    self.bolts.push(Bolt {
                        pos: (
                            self.pilot.0 + dr + (dc != 0) as i16 * spread,
                            self.pilot.1 + dc + (dr != 0) as i16 * spread,
                        ),
                        dir: (dr, dc),
                        friendly: true,
                        life: barrel.range(),
                    });
                }
            }
            blade => {
                let _ = blade;
                // In a duel a cut is an exchange, not a swing at a bystander.
                let cut_damage = arm.damage();
                let cut_arc = arm.arc();
                if let Some(mut duel) = self.duel.take() {
                    if duel.lock > 0 {
                        duel.pressure += 1;
                    } else if duel.stagger > 0 {
                        // Nothing doing while you are off balance.
                    } else if duel.wind_up > 0 && duel.wind_up <= PARRY_WINDOW {
                        // Both blades arriving at once: they bind.
                        duel.lock = LOCK_TICKS;
                        duel.wind_up = 0;
                        duel.pressure = 0;
                    } else {
                        let opening = duel.foe_stagger > 0 || duel.riposte > 0;
                        let cut = if opening {
                            cut_damage * 2
                        } else if duel.stance == Stance::Defensive {
                            cut_damage / 2
                        } else {
                            cut_damage
                        };
                        if let Some(foe) = self.troopers.get_mut(duel.foe) {
                            foe.hp -= cut;
                        }
                        duel.riposte = 0;
                        if !opening {
                            duel.foe_poise -= 1;
                            if duel.foe_poise <= 0 {
                                duel.foe_stagger = STAGGER_TICKS;
                                duel.foe_poise = DUEL_POISE;
                            }
                        }
                    }
                    self.duel = Some(duel);
                    self.troopers.retain(|t| t.hp > 0);
                    return true;
                }
                // A cut takes the two cells in front and either side of them.
                for reach in 1..=SABRE_REACH {
                    for across in -cut_arc..=cut_arc {
                        let cell = (
                            self.pilot.0 + dr * reach + (dc != 0) as i16 * across,
                            self.pilot.1 + dc * reach + (dr != 0) as i16 * across,
                        );
                        if let Some(hit) =
                            self.troopers.iter_mut().find(|t| t.hp > 0 && t.pos == cell)
                        {
                            hit.hp -= cut_damage;
                        }
                    }
                }
                self.troopers.retain(|t| t.hp > 0);
            }
        }
        true
    }

    /// Put your guard up. Time it against his wind-up and the cut comes back at
    /// him instead of landing on you.
    pub fn parry(&mut self) -> bool {
        if self.riding {
            return false;
        }
        match self.duel.as_mut() {
            Some(duel) if duel.stagger == 0 => {
                duel.guard = GUARD_TICKS;
                true
            }
            _ => false,
        }
    }

    /// Find somebody to cross blades with, or let one go when he is out of
    /// reach or down.
    fn measure_duel(&mut self) {
        let pilot = self.pilot;
        let near = self
            .troopers
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind == GroundKind::Duellist && t.hp > 0)
            .map(|(i, t)| (i, (t.pos.0 - pilot.0).abs() + (t.pos.1 - pilot.1).abs()))
            .filter(|(_, range)| *range <= DUEL_RANGE)
            .min_by_key(|(_, range)| *range);
        match (near, self.duel.as_ref()) {
            (Some((foe, _)), None) => {
                let stance = match foe % 3 {
                    0 => Stance::Aggressive,
                    1 => Stance::Defensive,
                    _ => Stance::Measured,
                };
                self.duel = Some(Duel::new(foe, stance));
            }
            (None, Some(_)) => self.duel = None,
            (Some((foe, _)), Some(duel)) if duel.foe != foe => {
                self.duel = Some(Duel::new(foe, duel.stance))
            }
            _ => {}
        }
    }

    /// One tick of the duel: his wind-up, your guard, the lock, and whatever
    /// lands. Returns what it cost you.
    fn trade_blows(&mut self) -> i32 {
        self.measure_duel();
        let Some(mut duel) = self.duel.take() else {
            return 0;
        };
        let Some(foe) = self.troopers.get(duel.foe).filter(|t| t.hp > 0) else {
            return 0;
        };
        let stance = duel.stance;
        let _ = foe;
        duel.guard = duel.guard.saturating_sub(1);
        duel.riposte = duel.riposte.saturating_sub(1);
        duel.stagger = duel.stagger.saturating_sub(1);
        duel.foe_stagger = duel.foe_stagger.saturating_sub(1);
        let mut cost = 0;
        if duel.lock > 0 {
            duel.lock -= 1;
            if duel.lock == 0 {
                // Whoever leaned into it harder walks away from it.
                if duel.pressure > 0 {
                    duel.foe_stagger = STAGGER_TICKS;
                    duel.foe_poise -= 1;
                } else {
                    duel.stagger = STAGGER_TICKS;
                    duel.poise -= 1;
                }
                duel.pressure = 0;
            }
            self.duel = Some(duel);
            return 0;
        }
        if duel.foe_stagger == 0 {
            if duel.wind_up == 0 {
                duel.wind_up = stance.wind_up();
            } else {
                duel.wind_up -= 1;
                if duel.wind_up == 0 {
                    // The cut lands, or it is turned.
                    if duel.guard > 0 {
                        duel.foe_poise -= 1;
                        duel.riposte = RIPOSTE_TICKS;
                        if duel.foe_poise <= 0 {
                            duel.foe_stagger = STAGGER_TICKS;
                            duel.foe_poise = DUEL_POISE;
                        }
                    } else {
                        cost = stance.damage();
                        duel.poise -= 1;
                        if duel.poise <= 0 {
                            duel.stagger = STAGGER_TICKS;
                            duel.poise = DUEL_POISE;
                        }
                    }
                }
            }
        }
        self.duel = Some(duel);
        cost
    }

    /// Put a particular thing in the pilot's hands.
    pub fn hold(&mut self, arm: SideArm) {
        self.sidearm = arm;
        self.blaster_cooldown = self.blaster_cooldown.max(arm.cadence());
    }

    /// Get on or off the speeder parked beside you.
    pub fn mount(&mut self) -> bool {
        self.riding = !self.riding;
        self.riding
    }

    /// A tick of the firefight: bolts fly, troopers advance and shoot back.
    pub fn skirmish(&mut self, tick: u32) -> i32 {
        self.blaster_cooldown = self.blaster_cooldown.saturating_sub(1);
        let mut damage = 0;
        let mut deflected: Vec<Bolt> = Vec::new();
        let pilot = self.pilot;
        // Bolts first, so a trooper that just fired does not eat his own shot.
        let mut kept = Vec::with_capacity(self.bolts.len());
        'bolt: for mut bolt in std::mem::take(&mut self.bolts) {
            for _ in 0..2 {
                bolt.pos.0 += bolt.dir.0;
                bolt.pos.1 += bolt.dir.1;
                bolt.life = bolt.life.saturating_sub(1);
                if bolt.life == 0
                    || !(0..self.height).contains(&bolt.pos.0)
                    || !(0..self.width).contains(&bolt.pos.1)
                    || self.blocked(bolt.pos.0, bolt.pos.1)
                {
                    continue 'bolt;
                }
                if bolt.friendly {
                    if let Some(hit) = self
                        .troopers
                        .iter_mut()
                        .find(|t| t.hp > 0 && t.pos == bolt.pos)
                    {
                        hit.hp -= 2;
                        continue 'bolt;
                    }
                } else if self.sidearm.is_blade()
                    && !self.riding
                    && (bolt.pos.0 - pilot.0).abs() <= 1
                    && (bolt.pos.1 - pilot.1).abs() <= 1
                {
                    // Turned back the way it came.
                    deflected.push(Bolt {
                        pos: bolt.pos,
                        dir: (-bolt.dir.0, -bolt.dir.1),
                        friendly: true,
                        life: BOLT_RANGE,
                    });
                    continue 'bolt;
                } else if bolt.pos == pilot {
                    damage += 1;
                    continue 'bolt;
                }
            }
            kept.push(bolt);
        }
        self.bolts = kept;
        self.bolts.extend(deflected);
        self.troopers.retain(|t| t.hp > 0);
        // Then the patrol: close the range, and fire when they have the angle.
        let mut volley = Vec::new();
        let mut melee = 0;
        for trooper in self.troopers.iter_mut() {
            let (dr, dc) = (pilot.0 - trooper.pos.0, pilot.1 - trooper.pos.1);
            let range = dr.abs() + dc.abs();
            if tick.is_multiple_of(3) && range > 4 {
                let step = if dr.abs() > dc.abs() {
                    (dr.signum(), 0)
                } else {
                    (0, dc.signum())
                };
                let next = (trooper.pos.0 + step.0, trooper.pos.1 + step.1);
                let solid = self.cover.iter().any(|block| {
                    (block.pos.0..block.pos.0 + block.size.0).contains(&next.0)
                        && (block.pos.1..block.pos.1 + block.size.1).contains(&next.1)
                });
                if !solid {
                    trooper.pos = next;
                }
            }
            if trooper.cooldown > 0 {
                trooper.cooldown -= 1;
                continue;
            }
            match trooper.kind {
                // A duellist does not shoot: he closes and cuts.
                GroundKind::Duellist => {
                    // He fights in the duel, not by walking into you.
                    if range > DUEL_RANGE {
                        trooper.cooldown = DUEL_CADENCE;
                    }
                }
                kind => {
                    if range <= kind.reach() && (dr == 0 || dc == 0) {
                        trooper.cooldown = if kind == GroundKind::Scout {
                            SCOUT_CADENCE
                        } else {
                            TROOPER_CADENCE
                        };
                        volley.push(Bolt {
                            pos: trooper.pos,
                            dir: (dr.signum(), dc.signum()),
                            friendly: false,
                            life: BOLT_RANGE,
                        });
                        if kind == GroundKind::Scout {
                            // The chin gun fires in pairs.
                            volley.push(Bolt {
                                pos: trooper.pos,
                                dir: (dr.signum(), dc.signum()),
                                friendly: false,
                                life: BOLT_RANGE,
                            });
                        }
                    }
                }
            }
        }
        self.bolts.extend(volley);
        // Blades are settled in their own exchange rather than by range alone.
        let duelled = self.trade_blows();
        damage + melee + duelled
    }

    /// Walk a step, staying on the deck.
    pub fn walk(&mut self, dc: i16, dr: i16) {
        if dc != 0 || dr != 0 {
            self.facing = (dr.signum(), dc.signum());
        }
        let col = (self.pilot.1 + dc).clamp(1, self.width - 2);
        let row = (self.pilot.0 + dr).clamp(1, self.height - 2);
        if !self.blocked(row, col) {
            self.pilot = (row, col);
        }
    }

    /// Whatever the pilot is standing at, if anything.
    pub fn at_hand(&self) -> Option<DeckSpot> {
        let (fr, fc) = self.facing;
        self.spots
            .iter()
            .filter(|(_, pos)| {
                let (dr, dc) = (pos.0 - self.pilot.0, pos.1 - self.pilot.1);
                let standing = dr.abs() <= Deck::REACH && dc.abs() <= Deck::REACH + 1;
                // Or a few paces straight in front of where you are looking.
                let ahead = dr * fr + dc * fc;
                let across = (dr * -fc + dc * fr).abs();
                standing || ((1..=Deck::SIGHT).contains(&ahead) && across <= 2)
            })
            .min_by_key(|(_, pos)| (pos.0 - self.pilot.0).abs() + (pos.1 - self.pilot.1).abs())
            .map(|(spot, _)| *spot)
    }

    /// Where a spot stands, for drawing it.
    pub fn spot_at(&self, spot: DeckSpot) -> Option<(i16, i16)> {
        self.spots
            .iter()
            .find(|(kind, _)| *kind == spot)
            .map(|(_, pos)| *pos)
    }
}

/// A world the fighting happens over. Most systems have one hanging in the
/// court; a surface mission is flown down on the deck of one.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Planet {
    Tatooine,
    Hoth,
    Yavin,
    Endor,
    Bespin,
    Kessel,
    Alderaan,
    Jakku,
    Scarif,
    Mustafar,
    Dagobah,
    Coruscant,
    Naboo,
    Geonosis,
    Mandalore,
    /// Nothing but stars out here.
    DeepSpace,
}

impl Planet {
    pub const ALL: [Planet; 16] = [
        Planet::Tatooine,
        Planet::Hoth,
        Planet::Yavin,
        Planet::Endor,
        Planet::Bespin,
        Planet::Kessel,
        Planet::Alderaan,
        Planet::Jakku,
        Planet::Scarif,
        Planet::Mustafar,
        Planet::Dagobah,
        Planet::Coruscant,
        Planet::Naboo,
        Planet::Geonosis,
        Planet::Mandalore,
        Planet::DeepSpace,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Planet::Tatooine => "Tatooine",
            Planet::Hoth => "Hoth",
            Planet::Yavin => "Yavin IV",
            Planet::Endor => "Endor",
            Planet::Bespin => "Bespin",
            Planet::Kessel => "Kessel",
            Planet::Alderaan => "Alderaan",
            Planet::Jakku => "Jakku",
            Planet::Scarif => "Scarif",
            Planet::Mustafar => "Mustafar",
            Planet::Dagobah => "Dagobah",
            Planet::Coruscant => "Coruscant",
            Planet::Naboo => "Naboo",
            Planet::Geonosis => "Geonosis",
            Planet::Mandalore => "Mandalore",
            Planet::DeepSpace => "deep space",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Planet::Tatooine => "twin suns over open desert",
            Planet::Hoth => "ice, and not much else",
            Planet::Yavin => "a gas giant with a moon full of Rebels",
            Planet::Endor => "forest moon under a half-built station",
            Planet::Bespin => "gas streams and floating cities",
            Planet::Kessel => "spice mines at the edge of the Maw",
            Planet::Alderaan => "what is left of it",
            Planet::Jakku => "a graveyard of hulls half-buried in sand",
            Planet::Scarif => "beaches, and a shield gate above them",
            Planet::Mustafar => "lava, ash and Imperial works",
            Planet::Dagobah => "swamp thick enough to hide in",
            Planet::Coruscant => "the whole world is one city",
            Planet::Naboo => "green plains and a hangar full of fighters",
            Planet::Geonosis => "red rock and droid foundries",
            Planet::Mandalore => "grey dust and old armour",
            Planet::DeepSpace => "nothing out here but stars",
        }
    }

    /// The shading the disc is drawn with; ice reads bright, lava dark.
    pub fn shade(self) -> &'static str {
        match self {
            Planet::Hoth | Planet::Bespin | Planet::Naboo => "░",
            Planet::Tatooine | Planet::Jakku | Planet::Geonosis | Planet::Scarif => "▒",
            Planet::DeepSpace => " ",
            _ => "▓",
        }
    }

    /// Whether a mission here is flown down on the deck rather than in orbit.
    pub fn surface(self) -> bool {
        matches!(
            self,
            Planet::Hoth | Planet::Endor | Planet::Scarif | Planet::Geonosis
        )
    }

    /// Which world hangs over a given stretch of space.
    pub fn of_sector(sector: Sector) -> Planet {
        match sector {
            Sector::SolarCorona => Planet::Tatooine,
            Sector::AsteroidBelt => Planet::Hoth,
            Sector::Nebula => Planet::Kessel,
            Sector::DebrisRing => Planet::Alderaan,
            Sector::Wreckage => Planet::Jakku,
            Sector::CometTrail => Planet::Bespin,
            Sector::VoidRift => Planet::Mustafar,
            Sector::Minefield => Planet::Scarif,
            Sector::IonStorm => Planet::Geonosis,
            Sector::OpenSpace => Planet::DeepSpace,
        }
    }
}

/// What a mission actually asks of the squadron.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Objective {
    /// Clear the system: everything Imperial in it has to go.
    Destroy,
    /// Get the transports out: this many have to reach the far side.
    Escort { needed: usize },
    /// Hold out for this long, whatever comes.
    Survive { ticks: u32 },
    /// Bring the walkers down; the cables are the only thing that will do it.
    Walkers { count: usize },
    /// Down the trench and put one in the port.
    CoreRun,
}

impl Objective {
    pub fn label(self) -> String {
        match self {
            Objective::Destroy => "clear the system".to_string(),
            Objective::Escort { needed } => format!("get {needed} transports through"),
            Objective::Survive { ticks } => format!("hold out for {}s", ticks / 14),
            Objective::Walkers { count } => format!("bring down {count} walkers"),
            Objective::CoreRun => "one shot down the shaft".to_string(),
        }
    }
}

/// One mission in the campaign: where it is fought, what is waiting, and what
/// counts as flying it properly.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mission {
    pub name: &'static str,
    pub briefing: &'static str,
    pub sector: Sector,
    pub terrain: TerrainKind,
    pub objective: Objective,
    pub capital: Option<CapitalKind>,
    pub boss: Option<BossKind>,
}

impl Mission {
    /// The campaign, flown in this order.
    pub const CAMPAIGN: [Mission; 8] = [
        Mission {
            name: "Tatooine Patrol",
            briefing: "A patrol out of the twin suns. Warm the cannons up on it.",
            sector: Sector::SolarCorona,
            terrain: TerrainKind::Open,
            objective: Objective::Destroy,
            capital: None,
            boss: None,
        },
        Mission {
            name: "The Kessel Run",
            briefing: "Twelve parsecs of nebula with the Maw pulling at you. Fly it.",
            sector: Sector::Nebula,
            terrain: TerrainKind::Tunnel,
            objective: Objective::Survive { ticks: 900 },
            capital: None,
            boss: None,
        },
        Mission {
            name: "Blockade Run",
            briefing: "A Star Destroyer is sitting on the lane. Punch a hole in it.",
            sector: Sector::OpenSpace,
            terrain: TerrainKind::Open,
            objective: Objective::Destroy,
            capital: Some(CapitalKind::StarDestroyer),
            boss: None,
        },
        Mission {
            name: "Battle of Yavin",
            briefing: "The station's shields are down to the trench. Use the Force.",
            sector: Sector::OpenSpace,
            terrain: TerrainKind::Trench,
            objective: Objective::CoreRun,
            capital: Some(CapitalKind::DeathStar),
            boss: None,
        },
        Mission {
            name: "Battle of Hoth",
            briefing: "Walkers on the north ridge. The cannons will not cut it — use the cables.",
            sector: Sector::AsteroidBelt,
            terrain: TerrainKind::Open,
            objective: Objective::Walkers { count: 3 },
            capital: None,
            boss: None,
        },
        Mission {
            name: "Evacuation of Hoth",
            briefing: "Transports are lifting off. Keep them alive until they jump.",
            sector: Sector::AsteroidBelt,
            terrain: TerrainKind::Open,
            objective: Objective::Escort { needed: 3 },
            capital: Some(CapitalKind::StarDestroyer),
            boss: None,
        },
        Mission {
            name: "Cloud City",
            briefing: "An ace is waiting in the gas streams. He knows you are coming.",
            sector: Sector::CometTrail,
            terrain: TerrainKind::Gates,
            objective: Objective::Destroy,
            capital: None,
            boss: Some(BossKind::AceTie),
        },
        Mission {
            name: "Battle of Endor",
            briefing: "The command ship first, then straight into the second station.",
            sector: Sector::DebrisRing,
            terrain: TerrainKind::Trench,
            objective: Objective::CoreRun,
            capital: Some(CapitalKind::DeathStar),
            boss: None,
        },
    ];
}

/// A transport running for the far side of the court.
#[derive(Clone, Debug)]
pub struct Transport {
    pub pos: (i16, i16),
    pub hp: i32,
    /// True once it has crossed and gone to lightspeed.
    pub away: bool,
}

/// An armoured walker on the surface. Cannons barely mark it; a cable wrapped
/// round its legs twice puts it down.
#[derive(Clone, Debug)]
pub struct Walker {
    pub pos: (i16, i16),
    pub hp: i32,
    /// Turns of cable round the legs.
    pub wraps: u32,
    pub cooldown: u32,
    pub down: bool,
}

impl Walker {
    /// Six cells of hull, drawn from the anchor.
    pub const SPAN: i16 = 3;

    fn new(pos: (i16, i16), armour: i32) -> Walker {
        Walker {
            pos,
            hp: 60 + armour * 8,
            wraps: 0,
            cooldown: 24,
            down: false,
        }
    }
}

/// Where the reactor's output is going. Six pips, split three ways: cannons hit
/// harder, deflectors knit themselves back, engines push the hull along and
/// charge the special faster.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Power {
    pub lasers: u32,
    pub shields: u32,
    pub engines: u32,
}

impl Default for Power {
    fn default() -> Self {
        Power {
            lasers: 2,
            shields: 2,
            engines: 2,
        }
    }
}

/// Which of the three systems power is being sent to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum System {
    Lasers,
    Shields,
    Engines,
}

impl System {
    pub fn name(self) -> &'static str {
        match self {
            System::Lasers => "lasers",
            System::Shields => "shields",
            System::Engines => "engines",
        }
    }
}

impl Power {
    fn pips(&self, system: System) -> u32 {
        match system {
            System::Lasers => self.lasers,
            System::Shields => self.shields,
            System::Engines => self.engines,
        }
    }

    fn add(&mut self, system: System, delta: i32) {
        let slot = match system {
            System::Lasers => &mut self.lasers,
            System::Shields => &mut self.shields,
            System::Engines => &mut self.engines,
        };
        *slot = (*slot as i32 + delta).clamp(0, POWER_PIPS as i32) as u32;
    }

    /// Take a pip off whichever system has most to spare and give it to `to`.
    pub fn divert(&mut self, to: System) -> bool {
        if self.pips(to) >= POWER_PIPS {
            return false;
        }
        let donor = [System::Lasers, System::Shields, System::Engines]
            .into_iter()
            .filter(|&s| s != to)
            .max_by_key(|&s| self.pips(s));
        match donor {
            Some(from) if self.pips(from) > 0 => {
                self.add(from, -1);
                self.add(to, 1);
                true
            }
            _ => false,
        }
    }
}

/// How the wing flies on you: the shapes a squadron actually uses.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum WingFormation {
    /// Everybody level with the leader, spread wide.
    LineAbreast,
    /// The vic: two back and out, then two further back and wider.
    Vic,
    /// All of them stacked back off one wing.
    Echelon,
    /// Line astern, one behind the other.
    Trail,
    /// Four corners around the leader.
    Box,
}

impl WingFormation {
    pub const ALL: [WingFormation; 5] = [
        WingFormation::LineAbreast,
        WingFormation::Vic,
        WingFormation::Echelon,
        WingFormation::Trail,
        WingFormation::Box,
    ];

    pub fn name(self) -> &'static str {
        match self {
            WingFormation::LineAbreast => "line abreast",
            WingFormation::Vic => "vic",
            WingFormation::Echelon => "echelon",
            WingFormation::Trail => "trail",
            WingFormation::Box => "box",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            WingFormation::LineAbreast => "wide and level — most guns forward",
            WingFormation::Vic => "back and out, the classic three",
            WingFormation::Echelon => "stacked off one wing for a firing pass",
            WingFormation::Trail => "line astern, narrow through a trench",
            WingFormation::Box => "four corners, covering each other",
        }
    }

    /// Where the wingman in `slot` rides, as `(rows back, columns out)`.
    pub fn offset(self, slot: usize) -> (i16, i16) {
        let n = slot as i16;
        let side = if slot.is_multiple_of(2) { -1 } else { 1 };
        let rank = n / 2 + 1;
        match self {
            WingFormation::LineAbreast => (0, side * 6 * rank),
            WingFormation::Vic => (rank, side * 6 * rank),
            WingFormation::Echelon => (rank, -6 * rank),
            WingFormation::Trail => ((n + 1) * 2, 0),
            WingFormation::Box => (if rank > 1 { 2 } else { 0 }, side * 8),
        }
    }
}

/// One hull in the squad: the one being flown, or a wingman riding alongside.
#[derive(Clone, Debug)]
pub struct Wing {
    pub name: &'static str,
    pub class: ShipClass,
    pub loadout: Loadout,
    pub weapon: Weapon,
    pub weapon_level: u32,
    pub shield: u32,
    pub max_shield: u32,
    /// False once it has been shot down; a yard puts it back in the air.
    pub alive: bool,
}

impl Wing {
    /// A fresh hull of `class`, straight off the line.
    pub fn new(name: &'static str, class: ShipClass) -> Wing {
        Wing {
            name,
            class,
            loadout: Loadout::default(),
            weapon: Weapon::LaserCannon,
            weapon_level: 1,
            shield: class.max_shield(),
            max_shield: class.max_shield(),
            alive: true,
        }
    }

    /// Damage this hull deals per shot.
    pub fn damage(&self) -> i32 {
        self.class.damage() + self.loadout.tier(Part::Cannon) as i32 + self.weapon_level as i32 - 1
    }

    pub fn status(&self) -> &'static str {
        if self.alive {
            "flying"
        } else {
            "down"
        }
    }
}

/// The names hulls are rolled off the line with.
const HULL_NAMES: [&str; MAX_SQUAD] = [
    "Red Leader",
    "Red Two",
    "Red Three",
    "Red Five",
    "Gold Leader",
    "Gold Two",
];

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
    /// Every hull in the squad, and which one is being flown.
    pub squad: Vec<Wing>,
    pub active: usize,
    /// The shape the wing flies in.
    pub wing_formation: WingFormation,
    /// Where the reactor's output is going.
    pub power: Power,
    /// How much of the Force is to hand, and what it is doing right now.
    pub force: u32,
    /// Ticks left of stretched-out senses.
    pub sense: u32,
    /// True while the next torpedo salvo flies itself.
    pub guided: bool,
    /// Squadron radio traffic, newest first.
    pub chatter: Vec<Chatter>,
    /// The hangar deck the pilot walks about on between missions, and whether
    /// the quartermaster's terminal is up.
    pub deck: Deck,
    pub shop_open: bool,
    /// The campaign mission being flown, and where the squadron is up to.
    pub mission: Option<Mission>,
    pub campaign_at: usize,
    /// The world this system belongs to.
    pub planet: Planet,
    /// What the mission wants, and the clock or count that tracks it.
    pub objective: Objective,
    pub objective_ticks: u32,
    /// Transports being escorted, and walkers to be brought down.
    pub transports: Vec<Transport>,
    pub walkers: Vec<Walker>,
    /// The guns aboard, and the missiles in the launcher.
    pub owned: Vec<Weapon>,
    /// What is on the pilot's belt, and what he has in his locker.
    pub armoury: Vec<SideArm>,
    pub missiles: u32,
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
    /// The capital ship holding this system, if one does.
    pub capital: Option<Capital>,
    /// The Alliance cruiser fighting alongside, if the fleet came out.
    pub ally: Option<Capital>,
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
    /// The galaxy being flown, its chart, and the system in front of the hull.
    pub galaxy: Galaxy,
    pub map: StarMap,
    pub node: MapNode,
    /// Ticks the arrival banner still has to run.
    pub banner: u32,
    /// Which system the bounty hunter is sitting in, if one has taken the
    /// contract yet.
    pub hunter: Option<usize>,
    /// How many times he has caught up with you.
    pub hunted: u32,
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
            class: ShipClass::XWing,
            difficulty: Difficulty::Normal,
            loadout: Loadout::default(),
            ship: (SHIP_ROW, W / 2),
            weapon: Weapon::LaserCannon,
            weapon_level: 1,
            shield: 0,
            max_shield: 0,
            lives: 3,
            bombs: 0,
            drones: Vec::new(),
            squad: vec![Wing::new(HULL_NAMES[0], ShipClass::XWing)],
            active: 0,
            wing_formation: WingFormation::Vic,
            power: Power::default(),
            force: FORCE_MAX / 2,
            sense: 0,
            guided: false,
            chatter: Vec::new(),
            deck: Deck::new(1),
            shop_open: false,
            mission: None,
            campaign_at: 0,
            planet: Planet::DeepSpace,
            objective: Objective::Destroy,
            objective_ticks: 0,
            transports: Vec::new(),
            walkers: Vec::new(),
            owned: vec![Weapon::LaserCannon],
            armoury: vec![SideArm::ServicePistol],
            missiles: MISSILE_START,
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
            capital: None,
            ally: None,
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
            galaxy: Galaxy::Orion,
            map: StarMap::generate(Galaxy::Orion, seed),
            node: MapNode {
                pos: (0, 0),
                region: Region::Rim,
                kind: NodeKind::Battle,
                sector: Sector::of_wave(1),
                terrain: TerrainKind::Open,
                bonus: NodeBonus::Refit,
                cleared: false,
                explored: true,
            },
            banner: 0,
            hunter: None,
            hunted: 0,
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
    pub fn start(&mut self, class: ShipClass, difficulty: Difficulty, galaxy: Galaxy) {
        self.class = class;
        self.difficulty = difficulty;
        self.galaxy = galaxy;
        self.loadout = Loadout::default();
        self.ship = (SHIP_ROW, W / 2);
        self.weapon = Weapon::LaserCannon;
        self.weapon_level = 1;
        self.bonus_plating = 0;
        self.bonus_damage = 0;
        self.bonus_regen = 0;
        self.recompute_shield();
        self.shield = self.max_shield;
        self.bombs = class.bombs();
        self.drones.clear();
        self.squad = vec![Wing::new(HULL_NAMES[0], class)];
        self.active = 0;
        self.owned = vec![Weapon::LaserCannon];
        self.armoury = vec![SideArm::ServicePistol];
        self.missiles = MISSILE_START;
        self.power = Power::default();
        self.force = FORCE_MAX / 2;
        self.sense = 0;
        self.guided = false;
        self.chatter.clear();
        self.deck = Deck::new(1);
        self.shop_open = false;
        self.mission = None;
        self.campaign_at = 0;
        self.hunter = None;
        self.hunted = 0;
        self.objective = Objective::Destroy;
        self.transports.clear();
        self.walkers.clear();
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
        let seed = self.rand();
        self.map = StarMap::generate(galaxy, seed);
        self.node = MapNode {
            pos: (0, 0),
            region: Region::Rim,
            kind: NodeKind::Battle,
            sector: self.galaxy.sectors()[0],
            terrain: TerrainKind::Open,
            bonus: NodeBonus::Refit,
            cleared: false,
            explored: true,
        };
        // The run opens parked at the rim, reading the chart.
        self.status = Status::Chart;
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
        let elite = if self.node.kind == NodeKind::Elite {
            2
        } else {
            0
        };
        (self.difficulty.armour() + (self.wave / 3) as i32 + danger + elite + self.galaxy.armour())
            .max(0)
    }

    /// Columns the hull covers per keypress, engine included.
    pub fn thrust(&self) -> i16 {
        self.class.speed()
            + (self.loadout.tier(Part::Engine) / 2) as i16
            + (self.power.engines / 3) as i16
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
        (self.class.damage()
            + self.loadout.tier(Part::Cannon) as i32
            + self.bonus_damage
            + self.power.lasers as i32
            - 2)
        .max(1)
    }

    /// Energy recovered per tick, reactor included.
    pub fn regen(&self) -> u32 {
        ENERGY_REGEN + self.loadout.tier(Part::Reactor) + self.bonus_regen + self.power.engines / 2
    }

    /// The rank the Alliance has you at.
    pub fn rank(&self) -> Rank {
        Rank::of_level(self.level)
    }

    /// Put a line of squadron traffic on the display.
    pub fn say(&mut self, line: &str) {
        if self.chatter.first().is_some_and(|c| c.line == line) {
            return;
        }
        self.chatter.insert(
            0,
            Chatter {
                line: line.to_string(),
                ticks: CHATTER_TICKS,
            },
        );
        self.chatter.truncate(3);
    }

    /// Reach for the Force. It only answers if there is enough of it.
    pub fn use_force(&mut self, power: ForcePower) -> bool {
        if self.status != Status::Playing || self.force < power.cost() {
            return false;
        }
        self.force -= power.cost();
        match power {
            ForcePower::Sense => {
                self.sense = SENSE_TICKS;
                self.say("Stretch out with your feelings.");
            }
            ForcePower::Pull => {
                let ship = self.ship;
                for pickup in self.powerups.iter_mut() {
                    pickup.pos = (ship.0, ship.1);
                }
                self.say("Pulling them in.");
            }
            ForcePower::Guided => {
                self.guided = true;
                self.say("Targeting computer off. Trusting the Force.");
            }
        }
        true
    }

    /// Send a pip of power to one of the three systems.
    pub fn divert(&mut self, system: System) -> bool {
        self.power.divert(system)
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
            0 if w >= 2 && col % 3 == 1 => EnemyKind::GunPlatform,
            0 if w >= 6 && col % 4 == 3 => EnemyKind::TieDefender,
            0 => EnemyKind::TieFighter,
            1 if w >= 3 && col % 4 == 2 => EnemyKind::TieBomber,
            1 if w >= 7 && col % 5 == 4 => EnemyKind::MineLayer,
            1 => EnemyKind::TieInterceptor,
            2 if w >= 5 && col.is_multiple_of(5) => EnemyKind::Gunboat,
            2 if w >= 9 && col % 4 == 2 => EnemyKind::RepairDroid,
            2 if w >= 6 && col % 3 == 1 => EnemyKind::VultureDroid,
            2 => EnemyKind::TieFighter,
            _ => EnemyKind::BuzzDroid,
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
        self.capital = None;
        self.ally = None;
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
        let arrival = match self.node.kind {
            NodeKind::Capital => "Heavy contact. That is no moon.",
            NodeKind::Boss => "Watch it — that one flies like he means it.",
            NodeKind::Elite => "Elite squadron on the scope. Lock S-foils.",
            _ => "All wings report in. Stay on target.",
        };
        self.say(arrival);
        self.planet = Planet::of_sector(self.sector);
        self.dress_sector();
        self.dress_terrain();
        self.dress_objective();
        self.claim_node_bonus();
        if self.hunted_here() {
            // He does not fly a TIE, and he does not fly alone.
            self.hunted += 1;
            let hunter = Boss::new(BossKind::AceTie, 80 + 40 * self.hunted as i32);
            self.boss = Some(hunter);
            for col in (0..COLS).step_by(4) {
                let home = self.place((FORMATION_TOP + 6, BASE_X + col as i16 * ENEMY_GAP));
                let escort = self.hatch(EnemyKind::TieAdvanced, home);
                self.enemies.push(escort);
            }
            self.say("That is the hunter. He brought friends.");
        }
        if self.node.kind == NodeKind::Capital {
            // A trench means a battlestation; open plating means a wedge, and
            // the rim only ever fields a picket.
            let kind = if self.node.terrain == TerrainKind::Trench {
                CapitalKind::DeathStar
            } else if self.node.region >= Region::Deep {
                CapitalKind::SuperDestroyer
            } else if self.node.region >= Region::Reach {
                CapitalKind::StarDestroyer
            } else {
                CapitalKind::ImperialFrigate
            };
            let armour = self.wave_armour();
            let capital = Capital::new(kind, armour, 40 * self.wave as i32);
            self.capital = Some(capital);
            // Against a wedge or bigger, the fleet comes out with you.
            if !matches!(kind, CapitalKind::ImperialFrigate) {
                let mut ally =
                    Capital::new(CapitalKind::MonCalamari, armour, 20 * self.wave as i32);
                ally.pos = (H - 5, W / 3);
                self.ally = Some(ally);
                self.say("Alliance cruiser is with us. Screen her.");
            }
            // A screen of fighters comes out with it.
            for col in (0..COLS).step_by(3) {
                let home = self.place((FORMATION_TOP + 9, BASE_X + col as i16 * ENEMY_GAP));
                let escort = self.hatch(EnemyKind::TieAdvanced, home);
                self.enemies.push(escort);
            }
            return;
        }
        if self.node.kind == NodeKind::Boss {
            let kind = BossKind::of_wave(self.wave);
            let hp = 60 + 40 * (self.wave / BOSS_EVERY) as i32 + 20 * self.wave_armour();
            self.boss = Some(Boss::new(kind, hp));
            for col in (0..COLS).step_by(2) {
                let home = self.place((FORMATION_TOP + 5, BASE_X + col as i16 * ENEMY_GAP));
                let escort = self.hatch(EnemyKind::BuzzDroid, home);
                self.enemies.push(escort);
            }
            return;
        }
        // Later waves come deeper as well as tougher, but a tight map fields a
        // narrower wave: there is only so much room between the rock.
        let rows = (2 + (self.wave / 3) as usize + self.galaxy.swarm()).min(ROWS);
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

    /// Leave the hangar for the chart, then fly the first lane on it. Kept as
    /// one call for the places that just want the next fight.
    pub fn launch_next_wave(&mut self) {
        self.open_chart();
        let fights: Vec<usize> = self
            .map
            .reachable()
            .into_iter()
            .filter(|&n| self.map.nodes[n].kind.fights())
            .collect();
        if let Some(&next) = fights.first() {
            self.map.cursor = next;
        }
        self.jump();
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
        if self.danger() || self.node.kind == NodeKind::Elite {
            salvage *= 2;
        }
        salvage += salvage * self.galaxy.salvage_bonus() / 100;
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
            Weapon::LaserCannon => {
                let lanes: &[i16] = match level {
                    1 => &[0],
                    2 => &[-1, 1],
                    _ => &[-1, 0, 1],
                };
                for &dx in lanes {
                    self.launch(Shot::bolt((r, c + dx), 0, dmg + 1));
                }
            }
            Weapon::QuadLaser => {
                let lanes: &[i16] = if level >= 2 {
                    &[-2, -1, 0, 1, 2]
                } else {
                    &[-1, 0, 1]
                };
                for &drift in lanes {
                    self.launch(Shot::bolt((r, c), drift, dmg));
                }
            }
            Weapon::HeavyLaser => {
                let lanes: &[i16] = match level {
                    1 => &[0],
                    2 => &[-1, 1],
                    _ => &[-1, 0, 1],
                };
                for &dx in lanes {
                    self.launch(Shot::beam((r, c + dx), dmg));
                }
            }
            Weapon::ConcussionMissile => {
                let lanes: &[i16] = match level {
                    1 => &[0],
                    2 => &[-2, 2],
                    _ => &[-2, 0, 2],
                };
                for &dx in lanes {
                    self.launch(Shot::missile((r, c + dx), dmg + 1));
                }
            }
            Weapon::ProtonBomb => {
                let half_width = if level >= 3 { 2 } else { 1 };
                self.launch(Shot::plasma((r, c), dmg + 2, half_width));
            }
            Weapon::RepeatingBlaster => {
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
            Weapon::RocketPod => {
                let lanes: &[i16] = match level {
                    1 => &[0],
                    2 => &[-2, 2],
                    _ => &[-2, 0, 2],
                };
                for &dx in lanes {
                    self.launch(Shot::rocket((r, c + dx), dmg + 2));
                }
            }
            Weapon::Flechette => {
                self.launch(Shot::flak((r, c), dmg + 1, FLAK_FUSE));
                if level >= 2 {
                    self.launch(Shot::flak((r, c - 3), dmg + 1, FLAK_FUSE + 2));
                }
                if level >= 3 {
                    self.launch(Shot::flak((r, c + 3), dmg + 1, FLAK_FUSE + 2));
                }
            }
            Weapon::MassDriver => {
                self.launch(Shot::rail((r, c), dmg * 3 + 2));
                if level >= 3 {
                    for dx in [-2, 2] {
                        self.launch(Shot::rail((r, c + dx), dmg * 2));
                    }
                }
            }
            Weapon::ArcCaster => {
                self.launch(Shot::arc((r, c), dmg + 1, level + 1));
            }
            Weapon::ProtonTorpedo => {
                // Two in the tube, three once it is tuned.
                let lanes: &[i16] = if level >= 3 { &[-3, 0, 3] } else { &[-3, 3] };
                for &dx in lanes {
                    self.launch(Shot::torpedo((r, c + dx), dmg * 4 + 6));
                }
            }
            Weapon::TowCable => {
                // The cable sweeps both ways; it is aimed at legs, not hulls.
                for drift in [-1, 1] {
                    self.launch(Shot::cable((r, c), drift));
                }
            }
            Weapon::IonCannon => {
                let lanes: &[i16] = match level {
                    1 => &[0],
                    2 => &[-2, 2],
                    _ => &[-2, 0, 2],
                };
                for &dx in lanes {
                    self.launch(Shot::ion((r, c + dx), dmg));
                }
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
            Special::Boost => {
                self.ship.1 = (self.ship.1 + self.facing * BLINK_DISTANCE).clamp(1, W - 2);
                self.invuln = self.invuln.max(BLINK_IFRAMES);
            }
            Special::Deflectors => self.bulwark = BULWARK_TICKS,
            Special::ProtonSalvo => {
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
            if self.shield == 0 {
                self.say("Deflectors gone!");
            }
            return;
        }
        self.lives = self.lives.saturating_sub(1);
        self.shield = self.max_shield;
        self.weapon_level = self.weapon_level.saturating_sub(1).max(1);
        self.drones.pop();
        self.say("I'm hit! Punching out — bring up the spare.");
    }

    /// Bank a kill at the current chain multiplier and extend the chain.
    fn award(&mut self, base: u32) {
        let points = base * self.combo;
        self.add_score(points);
        self.gain_xp(base);
        self.combo = (self.combo + 1).min(MAX_COMBO);
        self.combo_timer = COMBO_TICKS;
        self.force = (self.force + FORCE_PER_KILL).min(FORCE_MAX);
    }

    /// Pick up a dropped powerup.
    fn collect(&mut self, kind: PowerKind) {
        match kind {
            PowerKind::Gun(w) if w == self.weapon => {
                self.weapon_level = (self.weapon_level + 1).min(MAX_WEAPON_LEVEL);
            }
            PowerKind::Gun(w) => self.stock_gun(w),
            PowerKind::Missiles => self.missiles += MISSILE_PACK,
            PowerKind::EscapePod => {
                // A pilot picked up is a fighter back in the air.
                self.credits += 400;
                if let Some(wing) = self.squad.iter_mut().find(|w| !w.alive) {
                    wing.alive = true;
                    wing.shield = wing.max_shield;
                    let name = wing.name;
                    self.say(&format!("{name} is aboard. Get him a fighter."));
                } else {
                    self.lives += 1;
                    self.say("Pod recovered. That is a pilot we keep.");
                }
            }
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
            10 => PowerKind::Medal,
            11 => PowerKind::Missiles,
            15 => PowerKind::EscapePod,
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
        self.sense = self.sense.saturating_sub(1);
        if self.tick.is_multiple_of(FORCE_REGEN_TICKS) {
            self.force = (self.force + 1).min(FORCE_MAX);
        }
        for line in self.chatter.iter_mut() {
            line.ticks = line.ticks.saturating_sub(1);
        }
        self.chatter.retain(|c| c.ticks > 0);
        self.objective_ticks = self.objective_ticks.saturating_sub(1);
        self.energy = (self.energy + self.regen()).min(self.max_energy());
        if self.combo_timer > 0 {
            self.combo_timer -= 1;
            if self.combo_timer == 0 {
                self.combo = 1;
            }
        }
        if self.power.shields > 0
            && self.shield < self.max_shield
            && self
                .tick
                .is_multiple_of((SHIELD_KNIT_TICKS / self.power.shields).max(30))
        {
            self.shield += 1;
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
            .filter(|e| e.kind == EnemyKind::RepairDroid)
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
                    } else if e.kind == EnemyKind::TieInterceptor
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
                if e.kind == EnemyKind::BuzzDroid {
                    continue;
                }
                e.pos = (e.home.0, e.home.1 + sway);
                e.state = EnemyState::Formation;
            }
            if e.pos == ship {
                rammed = true;
                if e.kind == EnemyKind::BuzzDroid {
                    continue;
                }
            }
            if healing && e.kind != EnemyKind::RepairDroid && e.hp < e.max_hp {
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
                    EnemyKind::GunPlatform => {
                        spawned.push(Shot::enemy(muzzle, (ship.1 - e.pos.1).signum(), 1));
                    }
                    EnemyKind::TieBomber => {
                        for drift in [-1, 0, 1] {
                            spawned.push(Shot::enemy(muzzle, drift, 1));
                        }
                    }
                    // A fighter screen fires in pairs, spread a column apart.
                    EnemyKind::TieAdvanced => {
                        for drift in [-1, 1] {
                            spawned.push(Shot::enemy(muzzle, drift, 1));
                        }
                    }
                    EnemyKind::TieDefender => e.charge = SNIPER_CHARGE,
                    EnemyKind::MineLayer => mined.push(muzzle),
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
                BossKind::AceTie => {
                    // He leads the hull and jinks away from where it was.
                    let speed = if phase == 3 { 2 } else { 1 };
                    volley.push(Shot::enemy((row, boss.pos.1), aim, speed));
                    volley.push(Shot::enemy((row, boss.pos.1), aim * 2, speed));
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
                    let mut minion = self.hatch(EnemyKind::BuzzDroid, home);
                    minion.state = EnemyState::Diving { target_x: ship.1 };
                    self.enemies.push(minion);
                }
            }
        }
        self.boss = Some(boss);
    }

    /// Work the capital ship: hold station, run the batteries, launch fighters
    /// out of the bays and drag the hull in on the tractor beam.
    fn advance_capital(&mut self) {
        let Some(mut cap) = self.capital.take() else {
            return;
        };
        cap.tick = cap.tick.wrapping_add(1);
        for part in cap.parts.iter_mut() {
            part.ion = part.ion.saturating_sub(1);
        }
        // It creeps across the top of the court while its engines hold.
        if cap.under_way() && cap.tick.is_multiple_of(6) {
            let span = cap.kind.span(cap.kind.depth() - 1);
            let next = cap.pos.1 + cap.dir;
            if (span + 1..W - span - 1).contains(&next) {
                cap.pos.1 = next;
            } else {
                cap.dir = -cap.dir;
            }
        }
        let ship = self.ship;
        let cadence = cap.cadence();
        let mut volley: Vec<Shot> = Vec::new();
        let mut launches: Vec<(i16, i16)> = Vec::new();
        let mut pull = 0;
        for part in cap.parts.iter_mut() {
            if part.hp <= 0 || !part.live() {
                continue;
            }
            let (row, col) = (cap.pos.0 + part.offset.0, cap.pos.1 + part.offset.1);
            match part.kind {
                Emplacement::Turbolaser => {
                    if part.cooldown > 0 {
                        part.cooldown -= 1;
                    } else {
                        part.cooldown = cadence;
                        // Heavy batteries lead the hull and hit for two pips.
                        let aim = (ship.1 - col).signum();
                        volley.push(Shot::enemy((row + 1, col), aim, 1).heavy());
                        volley.push(Shot::enemy((row + 1, col), 0, 2).heavy());
                    }
                }
                Emplacement::HangarBay => {
                    if part.cooldown > 0 {
                        part.cooldown -= 1;
                    } else {
                        part.cooldown = cadence * 4;
                        launches.push((row + 1, col));
                    }
                }
                Emplacement::TractorBeam => {
                    if cap.tick.is_multiple_of(3) {
                        pull += (col - ship.1).signum();
                    }
                }
                Emplacement::GravityProjector if cap.tick.is_multiple_of(2) => {
                    pull += (col - ship.1).signum() * 2;
                }
                _ => {}
            }
        }
        for shot in volley {
            self.launch_enemy(shot);
        }
        for (row, col) in launches {
            for dx in [-2, 2] {
                let home = (row.min(H - 2), (col + dx).clamp(1, W - 2));
                let mut fighter = self.hatch(EnemyKind::TieAdvanced, home);
                fighter.state = EnemyState::Diving { target_x: ship.1 };
                self.enemies.push(fighter);
            }
        }
        if pull != 0 {
            let (left, right) = self.terrain.channel(self.ship.0);
            self.ship.1 = (self.ship.1 + pull).clamp(left.max(1), right.min(W - 2));
        }
        self.capital = Some(cap);
    }

    /// Work the Alliance cruiser: she holds station low, fires up the court and
    /// can be lost like anything else.
    fn advance_ally(&mut self) {
        let Some(mut ally) = self.ally.take() else {
            return;
        };
        ally.tick = ally.tick.wrapping_add(1);
        if ally.tick.is_multiple_of(10) {
            let next = ally.pos.1 + ally.dir;
            if (14..W - 14).contains(&next) {
                ally.pos.1 = next;
            } else {
                ally.dir = -ally.dir;
            }
        }
        let mut volley = Vec::new();
        for part in ally.parts.iter_mut() {
            if part.hp <= 0 || part.kind != Emplacement::Turbolaser {
                continue;
            }
            if part.cooldown > 0 {
                part.cooldown -= 1;
                continue;
            }
            part.cooldown = ALLY_CADENCE;
            let (row, col) = (ally.pos.0 + part.offset.0, ally.pos.1 + part.offset.1);
            let mut shell = Shot::bolt((row - 1, col), 0, ALLY_DAMAGE);
            shell.splash = 1;
            volley.push(shell);
        }
        for shot in volley {
            self.launch(shot);
        }
        if ally.hp <= 0 {
            self.say("They got the cruiser. We are on our own.");
            self.ally = None;
            return;
        }
        self.ally = Some(ally);
    }

    /// Imperial fire that reaches the cruiser chews on her instead of us.
    fn shell_ally(&mut self, pos: (i16, i16), damage: i32) -> bool {
        let Some(ally) = self.ally.as_mut() else {
            return false;
        };
        let hit = ally.covers(pos.0, pos.1)
            || ally
                .parts
                .iter()
                .any(|p| p.hp > 0 && ally.part_cell(p) == pos);
        if hit {
            ally.hp -= damage;
        }
        hit
    }

    /// Put a shot into a capital ship. Emplacements take it first; the hull is
    /// untouchable while a dome is up, and a hit on an open exhaust port takes
    /// the whole ship with it.
    fn hit_capital(&mut self, shot: &Shot) -> bool {
        let Some(mut cap) = self.capital.take() else {
            return false;
        };
        let (r, c) = shot.pos;
        let mut struck = false;
        let mut spoils: Vec<Emplacement> = Vec::new();
        let mut scuttled = false;
        let mut shaken = false;
        let shielded = cap.shielded();
        let reach = 1 + shot.half_width.max(shot.splash);
        for part in cap.parts.iter_mut() {
            if part.hp <= 0 {
                continue;
            }
            let (row, col) = (cap.pos.0 + part.offset.0, cap.pos.1 + part.offset.1);
            if (r - row).abs() > shot.splash || (c - col).abs() > reach {
                continue;
            }
            struck = true;
            if shot.ion {
                // An ion bolt scrambles rather than breaks: a dome that is out
                // is a hull that can be hit.
                part.ion = part.ion.max(ION_STUN);
                continue;
            }
            if part.kind == Emplacement::ExhaustPort && shielded {
                // The shaft is inside the envelope: the blast shakes the ship
                // but the port itself is untouched while the domes hold.
                shaken = true;
                continue;
            }
            part.hp -= shot.damage;
            if part.hp <= 0 {
                spoils.push(part.kind);
                if part.kind == Emplacement::ExhaustPort {
                    scuttled = true;
                }
            }
        }
        if !struck && cap.covers(r, c) {
            struck = true;
            if !shielded && !shot.ion {
                cap.hp -= shot.damage;
            }
        }
        if scuttled {
            // Straight down the shaft: the whole ship goes up.
            cap.hp = 0;
            self.say("Great shot, kid! That was one in a million.");
        } else if shaken {
            // The shields held the blast in, but it was felt.
            cap.hp -= cap.max_hp / 8;
        }
        self.capital = Some(cap);
        for kind in spoils {
            self.award(kind.score());
        }
        struck
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
        if shot.kind == ShotKind::Cable {
            // A cable does nothing to anything but legs.
            return self.snag_walker((r, c));
        }
        let mut hit = self.hit_boss(shot);
        if self.hit_capital(shot) {
            hit = true;
        }
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
            if kind == EnemyKind::VultureDroid {
                let target_x = self.ship.1;
                for dx in [-2, 2] {
                    let home = (pos.0, (pos.1 + dx).clamp(1, W - 2));
                    let mut half = self.hatch(EnemyKind::TieFighter, home);
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

    /// The weak point a guided salvo goes for: an open exhaust port first, then
    /// a shield generator, then whatever emplacement is nearest.
    pub fn weak_point(&self) -> Option<(i16, i16)> {
        let cap = self.capital.as_ref()?;
        let pick = |kind: Emplacement| {
            cap.parts
                .iter()
                .find(|p| p.kind == kind && p.hp > 0)
                .map(|p| cap.part_cell(p))
        };
        if !cap.shielded() {
            if let Some(port) = pick(Emplacement::ExhaustPort) {
                return Some(port);
            }
        }
        pick(Emplacement::ShieldDome)
            .or_else(|| pick(Emplacement::Turbolaser))
            .or_else(|| pick(Emplacement::HangarBay))
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
        let mut wing_hits: Vec<(usize, u32)> = Vec::new();
        let wings = self.wing_cells();
        let mut blocked: Vec<(i16, i16)> = Vec::new();
        let slowed = self.sense > 0;
        'shot: for mut s in std::mem::take(&mut self.enemy_shots) {
            if slowed && !self.tick.is_multiple_of(2) {
                // Everything crawls while the Force is up.
                kept.push(s);
                continue 'shot;
            }
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
                if self.shell_ally(s.pos, s.damage.max(1)) {
                    continue 'shot;
                }
                if s.pos == ship {
                    if !shielded {
                        hits.push(s.damage.max(1) as u32);
                    }
                    continue 'shot;
                }
                if let Some(&(index, _)) = wings.iter().find(|(_, cell)| *cell == s.pos) {
                    wing_hits.push((index, s.damage.max(1) as u32));
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
        for (index, pips) in wing_hits {
            self.damage_wing(index, pips);
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
            if self.hunted_here() {
                self.hunter = None;
                self.credits += HUNTER_BOUNTY;
                self.say("That is the hunter down. Somebody else will come.");
            }
            let bounty = 500 * self.wave;
            self.boss = None;
            self.add_score(bounty);
            self.gain_xp(bounty / 2);
        }
        if self.lives == 0 {
            self.status = Status::Lost;
            return;
        }
        if self.capital.as_ref().is_some_and(|c| c.hp <= 0) {
            let bounty = 2_000 + 400 * self.wave;
            self.capital = None;
            self.say("She's coming apart! Break off, break off.");
            self.add_score(bounty);
            self.gain_xp(bounty / 4);
        }
        // What counts as done depends on what was asked for.
        let swept = self.enemies.is_empty() && self.boss.is_none() && self.capital.is_none();
        let done = match self.objective {
            Objective::Destroy => swept,
            Objective::CoreRun => self.capital.is_none(),
            Objective::Survive { .. } => self.objective_ticks == 0,
            Objective::Walkers { .. } => {
                !self.walkers.is_empty() && self.walkers.iter().all(|w| w.down)
            }
            Objective::Escort { .. } => {
                !self.transports.is_empty() && self.transports.iter().all(|t| t.away || t.hp <= 0)
            }
        };
        if done && self.status == Status::Playing {
            if let Objective::Escort { needed } = self.objective {
                let away = self.transports.iter().filter(|t| t.away).count();
                if away >= needed {
                    self.say("All transports away. Good work.");
                    let bonus = 500 * away as u32;
                    self.add_score(bonus);
                } else {
                    self.say("We lost too many of them.");
                }
            }
        }
        if done && self.status == Status::Playing {
            self.status = Status::WaveClear;
            self.intermission = INTERMISSION_TICKS;
            self.map.clear_here();
            self.deck = Deck::new(self.squad.len());
            self.shop_open = false;
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
                Stock::Hull => self.squad.len() < MAX_SQUAD,
                Stock::Rescue => self.squad.iter().any(|w| !w.alive),
                _ => true,
            };
            let label = match stock {
                Stock::GunSwap => format!("swap gun ({} in the racks)", self.owned.len()),
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
                Stock::GunSwap => self.cycle_weapon(1),
                Stock::Drone => {
                    let side = if self.drones.contains(&-1) { 1 } else { -1 };
                    self.drones.push(side);
                }
                Stock::Bomb => self.bombs += 1,
                Stock::Rapid => self.rapid = RAPID_TICKS,
                Stock::Life => self.lives += 1,
                Stock::Missiles => self.missiles += MISSILE_PACK,
                Stock::Rescue => self.rescue_wings(),
                Stock::Hull => {
                    self.commission_hull();
                }
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

    /// Where each living wingman is riding, as `(squad index, cell)`.
    pub fn wing_cells(&self) -> Vec<(usize, (i16, i16))> {
        let mut cells = Vec::new();
        let mut slot = 0;
        for (i, wing) in self.squad.iter().enumerate() {
            if i == self.active || !wing.alive {
                continue;
            }
            if slot >= WING_SLOTS {
                break;
            }
            let (back, out) = self.wing_formation.offset(slot);
            slot += 1;
            // Behind the leader if there is room astern, mirrored ahead if the
            // leader is already on the deck line.
            let back = back.max(1);
            let row = if self.ship.0 + back <= SHIP_ROW {
                self.ship.0 + back
            } else {
                (self.ship.0 - back).max(1)
            };
            let (left, right) = self.terrain.channel(row);
            let col = (self.ship.1 + out).clamp(left.max(1), right.min(W - 2));
            cells.push((i, (row, col)));
        }
        cells
    }

    /// Order the wing into the next shape.
    pub fn cycle_formation(&mut self) -> WingFormation {
        let at = WingFormation::ALL
            .iter()
            .position(|&f| f == self.wing_formation)
            .unwrap_or(0);
        self.wing_formation = WingFormation::ALL[(at + 1) % WingFormation::ALL.len()];
        let shape = self.wing_formation;
        self.say(&format!("Form up — {}.", shape.name()));
        shape
    }

    /// The wingmen keep firing on their own, a little slower than the hull.
    fn advance_wings(&mut self) {
        if !self.tick.is_multiple_of(WING_CADENCE) {
            return;
        }
        let mut volley = Vec::new();
        for (i, (row, col)) in self.wing_cells() {
            let damage = self.squad[i].damage();
            volley.push(Shot::bolt((row - 1, col), 0, damage));
        }
        for shot in volley {
            self.launch(shot);
        }
    }

    /// A wingman takes a hit; at nothing left it is shot down and sits the rest
    /// of the run out until a yard puts it back in the air.
    fn damage_wing(&mut self, index: usize, pips: u32) {
        let Some(wing) = self.squad.get_mut(index) else {
            return;
        };
        let soaked = wing.shield.min(pips.max(1));
        wing.shield -= soaked;
        if wing.shield == 0 {
            wing.alive = false;
            let name = wing.name;
            self.flash = self.flash.max(4);
            self.say(&format!("{name} is hit! {name}, come in!"));
        }
    }

    /// Put every downed hull back in the air with full shields.
    pub fn rescue_wings(&mut self) {
        for wing in self.squad.iter_mut() {
            wing.alive = true;
            wing.shield = wing.max_shield;
        }
    }

    /// Store the hull being flown back into the squad roster.
    fn stow_active(&mut self) {
        let (weapon, level, shield, max_shield, loadout, class) = (
            self.weapon,
            self.weapon_level,
            self.shield,
            self.max_shield,
            self.loadout.clone(),
            self.class,
        );
        if let Some(wing) = self.squad.get_mut(self.active) {
            wing.weapon = weapon;
            wing.weapon_level = level;
            wing.shield = shield;
            wing.max_shield = max_shield;
            wing.loadout = loadout;
            wing.class = class;
        }
    }

    /// Climb into another hull in the squad. Only in the hangar, and only into
    /// one that is still flying.
    pub fn switch_active(&mut self, index: usize) -> bool {
        if self.status != Status::Hangar || index >= self.squad.len() || index == self.active {
            return false;
        }
        if !self.squad[index].alive {
            return false;
        }
        self.stow_active();
        let wing = self.squad[index].clone();
        self.active = index;
        self.class = wing.class;
        self.loadout = wing.loadout;
        self.weapon = wing.weapon;
        self.weapon_level = wing.weapon_level;
        self.bonus_plating = 0;
        self.recompute_shield();
        self.shield = wing.shield.min(self.max_shield);
        true
    }

    /// Climb into the next hull along.
    pub fn cycle_active(&mut self) -> bool {
        let count = self.squad.len();
        for step in 1..=count {
            let next = (self.active + step) % count;
            if self.switch_active(next) {
                return true;
            }
        }
        false
    }

    /// Take on a hull the squad does not have yet.
    fn commission_hull(&mut self) -> bool {
        if self.squad.len() >= MAX_SQUAD {
            return false;
        }
        let flown: Vec<ShipClass> = self.squad.iter().map(|w| w.class).collect();
        // A new type while the line has one spare, otherwise another of
        // whatever the squadron already flies.
        let class = ShipClass::ALL
            .into_iter()
            .find(|c| !flown.contains(c))
            .unwrap_or(ShipClass::ALL[self.squad.len() % ShipClass::ALL.len()]);
        let name = HULL_NAMES[self.squad.len()];
        self.squad.push(Wing::new(name, class));
        true
    }

    /// Put a gun in the racks and fit it.
    fn stock_gun(&mut self, weapon: Weapon) {
        if !self.owned.contains(&weapon) {
            self.owned.push(weapon);
        }
        self.weapon = weapon;
    }

    /// Swap to a gun already in the racks, by its slot on the HUD.
    pub fn select_weapon(&mut self, index: usize) -> bool {
        let Some(&weapon) = self.owned.get(index) else {
            return false;
        };
        self.weapon = weapon;
        true
    }

    /// Step through the racks one gun at a time.
    pub fn cycle_weapon(&mut self, delta: i32) {
        if self.owned.is_empty() {
            return;
        }
        let at = self
            .owned
            .iter()
            .position(|&w| w == self.weapon)
            .unwrap_or(0) as i32;
        let next = (at + delta).rem_euclid(self.owned.len() as i32) as usize;
        self.weapon = self.owned[next];
    }

    /// Fire the missile launcher: seeking rounds that blow a hole where they
    /// land, off their own ammunition rather than the gun cadence.
    pub fn fire_missiles(&mut self) {
        if self.status != Status::Playing || self.missiles == 0 {
            return;
        }
        self.missiles -= 1;
        let damage = if self.guided {
            self.gun_damage() * 3 + 12
        } else {
            self.gun_damage() + 4
        };
        let (r, c) = (self.ship.0 - 1, self.ship.1);
        // A guided salvo goes for the weak point rather than the nearest hull.
        let aim = self.guided.then(|| self.weak_point()).flatten();
        for slot in 0..MISSILE_SALVO {
            let dx = if slot % 2 == 0 { -2 } else { 2 };
            let mut missile = Shot::missile((r, c + dx), damage);
            missile.splash = 1;
            if let Some(target) = aim {
                missile.drift = (target.1 - (c + dx)).signum();
                missile.homing = false;
            }
            self.launch(missile);
        }
        if self.guided {
            self.guided = false;
        }
    }

    /// Walk about, on the hangar deck or down on a world. Not with a terminal
    /// open, and never mid-sortie.
    pub fn walk(&mut self, dc: i16, dr: i16) {
        if matches!(self.status, Status::Hangar | Status::Surface) && !self.shop_open {
            self.deck.walk(dc, dr);
        }
    }

    /// Shove whatever is in front of you off its feet, with the Force.
    pub fn force_push(&mut self) -> bool {
        if self.status != Status::Surface || self.force < PUSH_COST {
            return false;
        }
        self.force -= PUSH_COST;
        let (dr, dc) = self.deck.facing;
        let pilot = self.deck.pilot;
        let mut shoved = 0;
        for trooper in self.deck.troopers.iter_mut() {
            let (tr, tc) = (trooper.pos.0 - pilot.0, trooper.pos.1 - pilot.1);
            let ahead = tr * dr + tc * dc;
            let across = (tr * -dc + tc * dr).abs();
            if (1..=PUSH_REACH).contains(&ahead) && across <= 2 {
                trooper.pos = (
                    (trooper.pos.0 + dr * PUSH_REACH).clamp(1, Deck::HEIGHT - 2),
                    (trooper.pos.1 + dc * PUSH_REACH).clamp(1, Deck::WIDTH - 2),
                );
                trooper.hp -= 2;
                trooper.cooldown = trooper.cooldown.max(20);
                shoved += 1;
            }
        }
        self.deck.troopers.retain(|t| t.hp > 0);
        if shoved > 0 {
            self.say("Off their feet.");
        }
        true
    }

    /// Take the next thing off the belt.
    pub fn draw_sidearm(&mut self) -> Option<SideArm> {
        if !matches!(self.status, Status::Hangar | Status::Surface) || self.armoury.is_empty() {
            return None;
        }
        let at = self
            .armoury
            .iter()
            .position(|&arm| arm == self.deck.sidearm)
            .unwrap_or(0);
        let next = self.armoury[(at + 1) % self.armoury.len()];
        self.deck.hold(next);
        self.say(&format!("{} in hand.", next.name()));
        Some(next)
    }

    /// Buy something for the belt.
    pub fn buy_sidearm(&mut self, arm: SideArm) -> bool {
        if !matches!(self.status, Status::Hangar | Status::Surface)
            || self.armoury.contains(&arm)
            || self.credits < arm.price()
        {
            return false;
        }
        self.credits -= arm.price();
        self.armoury.push(arm);
        self.deck.hold(arm);
        self.say(&format!("{} on the belt.", arm.name()));
        true
    }

    /// Sell one back. Not the one in your hands, and never the last one.
    pub fn sell_sidearm(&mut self, arm: SideArm) -> bool {
        if !matches!(self.status, Status::Hangar | Status::Surface)
            || self.armoury.len() <= 1
            || self.deck.sidearm == arm
        {
            return false;
        }
        let Some(at) = self.armoury.iter().position(|&owned| owned == arm) else {
            return false;
        };
        self.armoury.remove(at);
        self.credits += arm.resale();
        self.say(&format!("Sold the {}.", arm.name()));
        true
    }

    /// What the quartermaster has on the rack, and what he will take back.
    pub fn armoury_lines(&self) -> Vec<(char, SideArm, u32, bool, bool)> {
        let keys = "qwertyuiopas";
        let mut lines = Vec::new();
        for (i, arm) in SideArm::ALL.iter().enumerate() {
            let Some(key) = keys.chars().nth(i) else {
                break;
            };
            let owned = self.armoury.contains(arm);
            let price = if owned { arm.resale() } else { arm.price() };
            let can = if owned {
                self.armoury.len() > 1 && self.deck.sidearm != *arm
            } else {
                self.credits >= arm.price()
            };
            lines.push((key, *arm, price, owned, can));
        }
        lines
    }

    /// Trade at the rack: buy what you do not have, sell back what you do.
    pub fn trade_sidearm(&mut self, key: char) -> bool {
        let Some(&(_, arm, _, owned, can)) = self.armoury_lines().iter().find(|line| line.0 == key)
        else {
            return false;
        };
        if !can {
            return false;
        }
        if owned {
            self.sell_sidearm(arm)
        } else {
            self.buy_sidearm(arm)
        }
    }

    /// Guard: get the blade up in time and his cut comes back at him.
    pub fn parry(&mut self) -> bool {
        if self.status != Status::Surface {
            return false;
        }
        self.deck.parry()
    }

    /// Get on or off the speeder.
    pub fn mount(&mut self) -> bool {
        if self.status != Status::Surface {
            return false;
        }
        let riding = self.deck.mount();
        self.say(if riding {
            "On the speeder. Watch the trees."
        } else {
            "Off the speeder."
        });
        riding
    }

    /// Fire the sidearm, on foot.
    pub fn shoot(&mut self) -> bool {
        if matches!(self.status, Status::Hangar | Status::Surface) && !self.shop_open {
            return self.deck.shoot();
        }
        false
    }

    /// A tick of the ground fight: bolts fly, the patrol closes, and a pilot
    /// who takes too much of it is carried back to his fighter.
    pub fn ground_tick(&mut self) {
        if self.status != Status::Surface || self.shop_open {
            return;
        }
        self.tick = self.tick.wrapping_add(1);
        let tick = self.tick;
        let hits = self.deck.skirmish(tick);
        if hits > 0 {
            self.deck.health -= hits;
            if self.deck.health <= 0 {
                self.deck.health = PILOT_HEALTH;
                self.lives = self.lives.saturating_sub(1);
                self.status = Status::Chart;
                self.say("Down there. They dragged him back to the ship.");
                return;
            }
            self.say("Taking fire on the ground!");
        }
        for line in self.chatter.iter_mut() {
            line.ticks = line.ticks.saturating_sub(1);
        }
        self.chatter.retain(|c| c.ticks > 0);
    }

    /// Turn on the spot, for the view out of the pilot's own eyes.
    pub fn turn(&mut self, dir: i16) {
        if matches!(self.status, Status::Hangar | Status::Surface) && !self.shop_open {
            self.deck.turn(dir);
        }
    }

    /// Walk the way you are facing.
    pub fn step_ahead(&mut self, ahead: i16) {
        if matches!(self.status, Status::Hangar | Status::Surface) && !self.shop_open {
            self.deck.step(ahead);
        }
    }

    /// Put down on the world under the system the cursor is on. Deep space has
    /// nothing to land on.
    pub fn land(&mut self) -> bool {
        if self.status != Status::Chart {
            return false;
        }
        let planet = Planet::of_sector(self.map.nodes[self.map.cursor].sector);
        if planet == Planet::DeepSpace {
            self.say("Nothing down there but vacuum.");
            return false;
        }
        let seed = self.rand();
        self.planet = planet;
        self.deck = Deck::surface(planet, seed);
        self.shop_open = false;
        self.status = Status::Surface;
        self.say(&format!("Down on {}. {}", planet.name(), planet.blurb()));
        true
    }

    /// Whatever the pilot is standing at right now.
    pub fn at_hand(&self) -> Option<DeckSpot> {
        matches!(self.status, Status::Hangar | Status::Surface)
            .then(|| self.deck.at_hand())
            .flatten()
    }

    /// Use whatever is under the pilot's hand.
    pub fn interact(&mut self) -> Option<DeckAction> {
        if !matches!(self.status, Status::Hangar | Status::Surface) {
            return None;
        }
        if self.shop_open {
            self.shop_open = false;
            return None;
        }
        let action = match self.deck.at_hand()? {
            DeckSpot::Bay(index) => {
                if index == self.active {
                    self.say("She is fuelled and ready.");
                    DeckAction::Boarded(index)
                } else if self.switch_active(index) {
                    let name = self.squad[index].name;
                    self.say(&format!("{name} is yours. Climb in."));
                    DeckAction::Boarded(index)
                } else {
                    self.say("That one is not going anywhere. Get the droids on it.");
                    return None;
                }
            }
            DeckSpot::Quartermaster => {
                self.shop_open = true;
                DeckAction::OpenedShop
            }
            DeckSpot::Navicomputer => {
                self.open_chart();
                DeckAction::OpenedChart
            }
            DeckSpot::AstromechPit => {
                self.rescue_wings();
                self.shield = self.max_shield;
                self.say("Droids have the wing back together.");
                DeckAction::Repaired
            }
            DeckSpot::BriefingTable => {
                let line = match self.mission {
                    Some(mission) => mission.briefing.to_string(),
                    None => format!(
                        "Chart says {} next. {}",
                        self.map.nodes[self.map.cursor].kind.name(),
                        self.map.nodes[self.map.cursor].sector.blurb()
                    ),
                };
                self.say(&line);
                DeckAction::ReadBriefing
            }
            DeckSpot::LaunchPad => {
                if self.mission.is_some() {
                    self.fly_mission(self.campaign_at + 1);
                } else {
                    self.open_chart();
                }
                DeckAction::Launched
            }
            DeckSpot::ParkedShip => {
                self.status = Status::Chart;
                self.say("Lifting off.");
                DeckAction::LiftedOff
            }
            DeckSpot::Cantina => {
                // Somebody in here always knows where the Empire is.
                let unknown: Vec<usize> = self
                    .map
                    .nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| !n.explored)
                    .map(|(i, _)| i)
                    .take(6)
                    .collect();
                for i in unknown {
                    self.map.nodes[i].explored = true;
                }
                self.credits += 60;
                self.say("Word in here puts a few more systems on the chart.");
                DeckAction::Drank
            }
            DeckSpot::SurfaceWreck => {
                let haul = 300 + 40 * self.wave;
                self.credits += haul;
                self.say(&format!("Stripped the wreck. {haul} in salvage."));
                DeckAction::Stripped
            }
            DeckSpot::Settlement => {
                self.shop_open = true;
                DeckAction::Traded
            }
            DeckSpot::Outpost => {
                self.shield = self.max_shield;
                self.missiles += MISSILE_PACK / 2;
                self.energy = self.max_energy();
                self.say("Outpost topped us up.");
                DeckAction::Resupplied
            }
            DeckSpot::Ruins => {
                // Older than the war, and there is something in them.
                self.force = FORCE_MAX;
                if self.rand().is_multiple_of(2) {
                    let gun = Weapon::ALL[(self.rand() % Weapon::ALL.len() as u64) as usize];
                    self.stock_gun(gun);
                    self.say("Something in the ruins. A gun, and the Force is strong here.");
                } else {
                    self.say("The Force is strong in this place.");
                }
                DeckAction::Explored
            }
        };
        Some(action)
    }

    /// Fly the campaign instead of the open galaxy: the missions from the war,
    /// in the order they were fought.
    pub fn start_campaign(&mut self, class: ShipClass, difficulty: Difficulty) {
        self.start(class, difficulty, Galaxy::Orion);
        self.campaign_at = 0;
        self.fly_mission(0);
    }

    /// Set up the campaign mission at `index` and launch straight into it.
    pub fn fly_mission(&mut self, index: usize) {
        let Some(&mission) = Mission::CAMPAIGN.get(index) else {
            // The war is over; what is left is the ceremony.
            self.mission = None;
            self.status = Status::Ceremony;
            self.say("That is the last of them. The fleet is standing down.");
            return;
        };
        self.campaign_at = index;
        self.mission = Some(mission);
        self.objective = mission.objective;
        self.wave = index as u32 + 1;
        self.node = MapNode {
            pos: (index, 0),
            region: Region::of_column(index, Mission::CAMPAIGN.len()),
            kind: if mission.capital.is_some() {
                NodeKind::Capital
            } else if mission.boss.is_some() {
                NodeKind::Boss
            } else {
                NodeKind::Battle
            },
            sector: mission.sector,
            terrain: mission.terrain,
            bonus: NodeBonus::Refit,
            cleared: false,
            explored: true,
        };
        self.status = Status::Playing;
        self.spawn_wave();
        self.say(mission.briefing);
    }

    /// Lay out whatever the mission's objective needs on top of the wave.
    fn dress_objective(&mut self) {
        self.transports.clear();
        self.walkers.clear();
        self.objective_ticks = 0;
        match self.objective {
            Objective::Survive { ticks } => self.objective_ticks = ticks,
            Objective::Escort { needed } => {
                for i in 0..needed + 1 {
                    let row = SHIP_TOP - 2 - (i as i16 % 3);
                    let col = 4 + i as i16 * 6;
                    self.transports.push(Transport {
                        pos: (row, col.min(W - 3)),
                        hp: 18 + self.wave_armour() * 3,
                        away: false,
                    });
                }
            }
            Objective::Walkers { count } => {
                for i in 0..count {
                    let col = 8 + i as i16 * 18;
                    let walker = Walker::new((H - 3, col.min(W - 6)), self.wave_armour());
                    self.walkers.push(walker);
                }
            }
            _ => {}
        }
    }

    /// Run the transports for the far side, and let the Empire shoot at them.
    fn advance_transports(&mut self) {
        if self.transports.is_empty() || !self.tick.is_multiple_of(3) {
            return;
        }
        let mut lost = 0;
        let mut away = 0;
        for transport in self.transports.iter_mut() {
            if transport.away || transport.hp <= 0 {
                continue;
            }
            transport.pos.1 += 1;
            if transport.pos.1 >= W - 2 {
                transport.away = true;
                away += 1;
            }
        }
        // Anything Imperial in the same cell chews on them.
        let cells: Vec<(usize, (i16, i16))> = self
            .transports
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.away && t.hp > 0)
            .map(|(i, t)| (i, t.pos))
            .collect();
        let mut hits: Vec<usize> = Vec::new();
        self.enemy_shots.retain(
            |shot| match cells.iter().find(|(_, pos)| *pos == shot.pos) {
                Some(&(i, _)) => {
                    hits.push(i);
                    false
                }
                None => true,
            },
        );
        for i in hits {
            self.transports[i].hp -= 3;
            if self.transports[i].hp <= 0 {
                lost += 1;
            }
        }
        if lost > 0 {
            self.say("They got one of the transports!");
        }
        if away > 0 {
            self.say("Transport away. Keep the rest of them alive.");
        }
    }

    /// Walk the walkers up the court and let them fire.
    fn advance_walkers(&mut self) {
        if self.walkers.is_empty() {
            return;
        }
        let ship = self.ship;
        let mut volley = Vec::new();
        for walker in self.walkers.iter_mut() {
            if walker.down {
                continue;
            }
            if walker.cooldown > 0 {
                walker.cooldown -= 1;
            } else {
                walker.cooldown = WALKER_CADENCE;
                volley.push(Shot::enemy((walker.pos.0 - 1, walker.pos.1), 0, 1).heavy());
            }
        }
        for shot in volley {
            self.launch_enemy(shot);
        }
        let _ = ship;
    }

    /// Put a turn of cable round a walker's legs; two turns and it goes over.
    fn snag_walker(&mut self, pos: (i16, i16)) -> bool {
        let hit = self.walkers.iter_mut().find(|w| {
            !w.down && (w.pos.0 - pos.0).abs() <= 1 && (w.pos.1 - pos.1).abs() <= Walker::SPAN
        });
        let Some(walker) = hit else {
            return false;
        };
        walker.wraps += 1;
        let down = walker.wraps >= CABLE_WRAPS;
        if down {
            walker.down = true;
        }
        if down {
            self.award(600);
            self.say("That got him! One down.");
        } else {
            self.say("Cable engaged — go around again.");
        }
        true
    }

    /// How many lanes a jump can cross. Gravity wells cut it back to one.
    pub fn jump_range(&self) -> usize {
        if self.interdicted() {
            return 1;
        }
        1 + (self.loadout.tier(Part::Hyperdrive) / 2) as usize
    }

    /// True while an Imperial gravity well is holding the squadron down: a
    /// capital with its projectors still up, here or one lane out.
    pub fn interdicted(&self) -> bool {
        if self
            .capital
            .as_ref()
            .is_some_and(|c| c.standing(Emplacement::GravityProjector) > 0)
        {
            return true;
        }
        self.map
            .reachable()
            .into_iter()
            .chain(std::iter::once(self.map.at))
            .any(|i| {
                let node = self.map.nodes[i];
                node.kind == NodeKind::Capital
                    && !node.cleared
                    && node.terrain != TerrainKind::Trench
            })
    }

    /// Open the galaxy chart from the hangar.
    pub fn open_chart(&mut self) {
        if matches!(self.status, Status::Hangar | Status::Chart) {
            self.status = Status::Chart;
        }
    }

    /// Move the chart cursor through the systems one lane out.
    pub fn move_cursor(&mut self, delta: i32) {
        if self.status != Status::Chart {
            return;
        }
        let lanes = self.map.reachable_within(self.jump_range());
        if lanes.is_empty() {
            return;
        }
        let at = lanes
            .iter()
            .position(|&n| n == self.map.cursor)
            .unwrap_or(0) as i32;
        let next = (at + delta).rem_euclid(lanes.len() as i32) as usize;
        self.map.cursor = lanes[next];
    }

    /// Point the chart cursor at whichever system lies that way.
    pub fn steer_chart(&mut self, dc: i32, dr: i32) {
        if self.status == Status::Chart {
            self.map.steer(dc, dr);
        }
    }

    /// Move the bounty hunter one lane closer. He takes the contract once the
    /// squadron has made enough of a nuisance of itself, and after that he is
    /// always one system behind.
    fn advance_hunter(&mut self) {
        if self.hunter.is_none() && self.score > HUNTER_BOUNTY {
            // He starts as far from you as the chart allows.
            let far = self
                .map
                .nodes
                .iter()
                .enumerate()
                .max_by_key(|(_, node)| {
                    node.pos.0.abs_diff(self.map.nodes[self.map.at].pos.0)
                        + node.pos.1.abs_diff(self.map.nodes[self.map.at].pos.1)
                })
                .map(|(i, _)| i);
            self.hunter = far;
            self.say("Somebody has posted a bounty. There is a ship on our trail.");
            return;
        }
        let Some(at) = self.hunter else {
            return;
        };
        if at == self.map.at {
            return;
        }
        // One lane a jump, always toward you.
        let target = self.map.nodes[self.map.at].pos;
        let next = self.map.lanes[at].iter().copied().min_by_key(|&i| {
            let pos = self.map.nodes[i].pos;
            pos.0.abs_diff(target.0) + pos.1.abs_diff(target.1)
        });
        if let Some(next) = next {
            self.hunter = Some(next);
            if next == self.map.at {
                self.say("He has us. Turn and fight.");
            }
        }
    }

    /// True while the hunter is sitting in the same system as the squadron.
    pub fn hunted_here(&self) -> bool {
        self.hunter == Some(self.map.at)
    }

    /// Fly the lane to the system under the cursor. A fight starts the next
    /// wave; a depot or a derelict is worked over and parks the squad again.
    pub fn jump(&mut self) -> bool {
        if self.status != Status::Chart {
            return false;
        }
        let range = self.jump_range();
        let Some(node) = self.map.jump_within(range) else {
            self.say("The gravity wells have us. Nothing jumps until they are down.");
            return false;
        };
        self.node = node;
        self.drone_stun = 0;
        self.advance_hunter();
        match node.kind {
            NodeKind::Depot => {
                // A yard: shields filled, a bomb loaded, the wing back in the
                // air and the launcher topped up, all of it free.
                self.shield = self.max_shield;
                self.bombs += 1;
                self.missiles += MISSILE_PACK / 2;
                self.energy = self.max_energy();
                self.rescue_wings();
                self.map.clear_here();
                self.status = Status::Hangar;
                self.deck = Deck::new(self.squad.len());
                self.shop_open = false;
                self.say("Docking at the outpost. Get her patched up.");
            }
            NodeKind::Derelict => {
                // A hulk worth stripping: salvage, and whatever gun is aboard.
                let haul = 250 + 90 * self.wave;
                self.credits += haul;
                if let NodeBonus::Armoury(weapon) = node.bonus {
                    self.stock_gun(weapon);
                } else {
                    self.bombs += 1;
                }
                self.map.clear_here();
                self.status = Status::Hangar;
                self.deck = Deck::new(self.squad.len());
                self.shop_open = false;
            }
            _ => {
                self.wave += 1;
                self.shield = self.max_shield;
                self.energy = self.max_energy();
                self.status = Status::Playing;
                self.spawn_wave();
            }
        }
        true
    }

    /// Hand over whatever the chosen stop was carrying.
    fn claim_node_bonus(&mut self) {
        match self.node.bonus {
            NodeBonus::Cache(credits) => self.credits += credits,
            NodeBonus::Armoury(weapon) => {
                self.stock_gun(weapon);
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
                self.advance_capital();
                self.advance_ally();
                self.advance_transports();
                self.advance_walkers();
                self.advance_wings();
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

/// A single cell of the 3D canvas: what is drawn there, how far away it is, and
/// in what colour. The depth is what lets a near hull hide a far one.
#[derive(Clone, Copy)]
struct Cell {
    glyph: char,
    depth: f32,
    style: zmax_view::graphics::Style,
}

/// A depth-buffered canvas the size of the viewport. Everything in the
/// first-person views is drawn into one of these and blitted once, so whatever
/// is nearest wins every cell.
struct Canvas {
    w: i16,
    h: i16,
    cells: Vec<Option<Cell>>,
}

impl Canvas {
    fn new(w: i16, h: i16) -> Canvas {
        Canvas {
            w,
            h,
            cells: vec![None; (w.max(1) * h.max(1)) as usize],
        }
    }

    /// Draw a cell if nothing nearer is already there.
    fn plot(&mut self, x: i16, y: i16, depth: f32, glyph: char, style: zmax_view::graphics::Style) {
        if !(0..self.w).contains(&x) || !(0..self.h).contains(&y) || depth <= 0.0 {
            return;
        }
        let at = (y * self.w + x) as usize;
        let nearer = match self.cells[at] {
            Some(cell) => depth < cell.depth,
            None => true,
        };
        if nearer {
            self.cells[at] = Some(Cell {
                glyph,
                depth,
                style,
            });
        }
    }

    fn blit(&self, surface: &mut Surface, ox: u16, oy: u16) {
        for y in 0..self.h {
            for x in 0..self.w {
                if let Some(cell) = self.cells[(y * self.w + x) as usize] {
                    surface.set_string(
                        ox + x as u16,
                        oy + y as u16,
                        &cell.glyph.to_string(),
                        cell.style,
                    );
                }
            }
        }
    }
}

/// A pinhole camera. World axes are x to the right, y up and z forward; `yaw`
/// turns it about the vertical.
struct Camera {
    x: f32,
    y: f32,
    z: f32,
    yaw: f32,
    focal: f32,
    w: f32,
    h: f32,
}

impl Camera {
    /// World point to screen cell, with the distance that got it there.
    fn project(&self, x: f32, y: f32, z: f32) -> Option<(f32, f32, f32)> {
        let (sin, cos) = self.yaw.sin_cos();
        let dx = x - self.x;
        let dz = z - self.z;
        let right = dx * cos - dz * sin;
        let ahead = dx * sin + dz * cos;
        if ahead < 0.7 {
            return None;
        }
        let up = y - self.y;
        // Terminal cells are about twice as tall as they are wide, so the
        // vertical axis is squashed to keep things looking square.
        let sx = self.w / 2.0 + right * self.focal / ahead;
        let sy = self.h / 2.0 - up * self.focal * 0.5 / ahead;
        Some((sx, sy, ahead))
    }
}

/// One box of a solid model: its centre and its half-extents, in model units.
/// One box of a solid model: its centre and its half-extents, in model units.
#[derive(Clone, Copy)]
pub struct Solid {
    at: [f32; 3],
    half: [f32; 3],
    /// The glyph its faces are drawn with before distance shading.
    face: char,
}

impl Solid {
    pub const fn new(at: [f32; 3], half: [f32; 3], face: char) -> Solid {
        Solid { at, half, face }
    }
}

/// How big a hull is drawn: model units are fighter-sized, not cell-sized, so a
/// TIE at ten cells fills a chunk of the glass rather than a couple of cells.
const HULL_SCALE: f32 = 2.6;

/// The shading ramp, darkest to brightest. A face's brightness comes from how
/// square it is to the light, dimmed by how far away it is.
const RAMP: [char; 7] = [' ', '·', '░', '▒', '▓', '█', '█'];

/// Where the light comes from: over your left shoulder, as in every hangar
/// photograph ever taken.
const LIGHT: [f32; 3] = [-0.45, 0.78, -0.44];

/// How lit a face is, from its normal, and how much the distance eats it.
fn face_light(normal: [f32; 3], depth: f32) -> f32 {
    let lit = normal[0] * LIGHT[0] + normal[1] * LIGHT[1] + normal[2] * LIGHT[2];
    let ambient = 0.5;
    let direct = lit.max(0.0) * 0.5;
    let fog = (1.0 - (depth / 70.0)).clamp(0.45, 1.0);
    ((ambient + direct) * fog).clamp(0.0, 1.0)
}

/// The glyph a surface of that brightness is drawn with.
fn ramp_glyph(light: f32) -> char {
    let step = (light * (RAMP.len() - 1) as f32).round() as usize;
    RAMP[step.min(RAMP.len() - 1)]
}

/// One flat face of a model, in world coordinates.
struct Quad {
    pts: [[f32; 3]; 4],
    normal: [f32; 3],
}

/// Rasterise a face: project its corners, walk the scanlines it covers, and
/// fill them at the depth and brightness the face has there.
fn fill_quad(
    canvas: &mut Canvas,
    cam: &Camera,
    quad: &Quad,
    style: zmax_view::graphics::Style,
    material: f32,
) {
    let mut screen = [(0.0f32, 0.0f32, 0.0f32); 4];
    for (i, point) in quad.pts.iter().enumerate() {
        match cam.project(point[0], point[1], point[2]) {
            // A face with a corner behind the lens is dropped rather than
            // clipped: at these sizes the difference is a cell or two.
            None => return,
            Some(projected) => screen[i] = projected,
        }
    }
    let top = screen.iter().fold(f32::MAX, |a, p| a.min(p.1));
    let bottom = screen.iter().fold(f32::MIN, |a, p| a.max(p.1));
    if bottom < 0.0 || top > canvas.h as f32 {
        return;
    }
    let lit = face_light(quad.normal, screen.iter().map(|p| p.2).sum::<f32>() / 4.0) * material;
    let glyph = ramp_glyph(lit);
    let mut y = top.floor().max(0.0);
    while y <= bottom.ceil().min(canvas.h as f32 - 1.0) {
        // Where the face's edges cross this scanline, and how deep it is there.
        let mut spans: Vec<(f32, f32)> = Vec::new();
        for i in 0..4 {
            let (ax, ay, ad) = screen[i];
            let (bx, by, bd) = screen[(i + 1) % 4];
            if (ay <= y && by > y) || (by <= y && ay > y) {
                let t = (y - ay) / (by - ay);
                spans.push((ax + (bx - ax) * t, ad + (bd - ad) * t));
            }
        }
        if spans.len() >= 2 {
            spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let (left, right) = (spans[0], spans[spans.len() - 1]);
            let mut x = left.0.floor().max(0.0);
            while x <= right.0.ceil().min(canvas.w as f32 - 1.0) {
                let t = if (right.0 - left.0).abs() < 0.001 {
                    0.0
                } else {
                    (x - left.0) / (right.0 - left.0)
                };
                let depth = left.1 + (right.1 - left.1) * t;
                canvas.plot(x as i16, y as i16, depth, glyph, style);
                x += 1.0;
            }
        }
        y += 1.0;
    }
}

/// Draw the outline of a face, which is what stops a hull reading as a blob.
fn edge_quad(canvas: &mut Canvas, cam: &Camera, quad: &Quad, style: zmax_view::graphics::Style) {
    let mut screen = [(0.0f32, 0.0f32, 0.0f32); 4];
    for (i, point) in quad.pts.iter().enumerate() {
        match cam.project(point[0], point[1], point[2]) {
            None => return,
            Some(projected) => screen[i] = projected,
        }
    }
    for i in 0..4 {
        let (ax, ay, ad) = screen[i];
        let (bx, by, bd) = screen[(i + 1) % 4];
        let steps = ((bx - ax).abs().max((by - ay).abs()) as i32).clamp(1, 200);
        for step in 0..=steps {
            let t = step as f32 / steps as f32;
            let x = ax + (bx - ax) * t;
            let y = ay + (by - ay) * t;
            let depth = ad + (bd - ad) * t;
            let glyph = if (by - ay).abs() < 0.6 {
                '─'
            } else if (bx - ax).abs() < 0.6 {
                '│'
            } else if (bx - ax) * (by - ay) > 0.0 {
                '╲'
            } else {
                '╱'
            };
            // Just in front of the face it belongs to, so it never z-fights.
            canvas.plot(x as i16, y as i16, depth - 0.05, glyph, style);
        }
    }
}

/// The six faces of a box, in world coordinates.
fn box_quads(centre: [f32; 3], half: [f32; 3]) -> [Quad; 6] {
    let (cx, cy, cz) = (centre[0], centre[1], centre[2]);
    let (hx, hy, hz) = (half[0], half[1], half[2]);
    let corner = |sx: f32, sy: f32, sz: f32| [cx + sx * hx, cy + sy * hy, cz + sz * hz];
    [
        Quad {
            pts: [
                corner(-1.0, -1.0, -1.0),
                corner(1.0, -1.0, -1.0),
                corner(1.0, 1.0, -1.0),
                corner(-1.0, 1.0, -1.0),
            ],
            normal: [0.0, 0.0, -1.0],
        },
        Quad {
            pts: [
                corner(-1.0, -1.0, 1.0),
                corner(1.0, -1.0, 1.0),
                corner(1.0, 1.0, 1.0),
                corner(-1.0, 1.0, 1.0),
            ],
            normal: [0.0, 0.0, 1.0],
        },
        Quad {
            pts: [
                corner(-1.0, 1.0, -1.0),
                corner(1.0, 1.0, -1.0),
                corner(1.0, 1.0, 1.0),
                corner(-1.0, 1.0, 1.0),
            ],
            normal: [0.0, 1.0, 0.0],
        },
        Quad {
            pts: [
                corner(-1.0, -1.0, -1.0),
                corner(1.0, -1.0, -1.0),
                corner(1.0, -1.0, 1.0),
                corner(-1.0, -1.0, 1.0),
            ],
            normal: [0.0, -1.0, 0.0],
        },
        Quad {
            pts: [
                corner(-1.0, -1.0, -1.0),
                corner(-1.0, 1.0, -1.0),
                corner(-1.0, 1.0, 1.0),
                corner(-1.0, -1.0, 1.0),
            ],
            normal: [-1.0, 0.0, 0.0],
        },
        Quad {
            pts: [
                corner(1.0, -1.0, -1.0),
                corner(1.0, 1.0, -1.0),
                corner(1.0, 1.0, 1.0),
                corner(1.0, -1.0, 1.0),
            ],
            normal: [1.0, 0.0, 0.0],
        },
    ]
}

/// Draw one box: its faces filled and lit, then its edges drawn over them.
fn draw_solid(
    canvas: &mut Canvas,
    cam: &Camera,
    solid: &Solid,
    origin: [f32; 3],
    scale: f32,
    style: zmax_view::graphics::Style,
) {
    // Panels and plating take the light differently from bare hull.
    let material = match solid.face {
        '█' => 1.0,
        '▓' => 0.85,
        _ => 0.7,
    };
    let centre = [
        origin[0] + solid.at[0] * scale,
        origin[1] + solid.at[1] * scale,
        origin[2] + solid.at[2] * scale,
    ];
    let half = [
        solid.half[0] * scale,
        solid.half[1] * scale,
        solid.half[2] * scale,
    ];
    let quads = box_quads(centre, half);
    for quad in &quads {
        fill_quad(canvas, cam, quad, style, material);
    }
    // Close work gets its outline; distant hulls are left as solids.
    let near = cam
        .project(centre[0], centre[1], centre[2])
        .is_some_and(|p| p.2 < 22.0);
    if near {
        for quad in &quads {
            edge_quad(canvas, cam, quad, style);
        }
    }
}

/// Draw a whole model: a handful of boxes bolted together.
fn draw_model(
    canvas: &mut Canvas,
    cam: &Camera,
    model: &[Solid],
    origin: [f32; 3],
    scale: f32,
    style: zmax_view::graphics::Style,
) {
    for solid in model {
        draw_solid(canvas, cam, solid, origin, scale, style);
    }
}

/// Sky and ground behind everything else: a lit band above the horizon and a
/// darker one below it, so the world has a floor and a ceiling before a single
/// hull is drawn.
fn draw_horizon(
    canvas: &mut Canvas,
    cam: &Camera,
    sky: char,
    ground: char,
    sky_style: zmax_view::graphics::Style,
    ground_style: zmax_view::graphics::Style,
) {
    let horizon = (cam.h / 2.0) as i16;
    for y in 0..canvas.h {
        let (glyph, style) = if y < horizon {
            (sky, sky_style)
        } else {
            (ground, ground_style)
        };
        // Scattered, not a grid: a cheap hash keeps it from reading as texture
        // wallpaper, and it thins out towards the horizon so that line reads.
        let band = (y - horizon).unsigned_abs() as u32;
        for x in 0..canvas.w {
            let hash = (x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663);
            let density = if band > 5 { 11 } else { 23 };
            if hash % density == 0 {
                canvas.plot(x, y, 900.0, glyph, style);
            }
        }
    }
    // The horizon itself, so there is a line between the two.
    for x in 0..canvas.w {
        canvas.plot(x, horizon, 890.0, '─', ground_style);
    }
}

/// The canopy itself: struts down the corners of the glass and a frame round
/// it, drawn nearest of all so it always sits in front of the fight.
fn draw_canopy(canvas: &mut Canvas, style: zmax_view::graphics::Style) {
    let (w, h) = (canvas.w, canvas.h);
    for x in 0..w {
        canvas.plot(x, 0, 0.01, '═', style);
        canvas.plot(x, h - 1, 0.01, '▁', style);
    }
    for y in 0..h {
        canvas.plot(0, y, 0.01, '║', style);
        canvas.plot(w - 1, y, 0.01, '║', style);
    }
    // Corner braces, the way a canopy actually frames the view: short runs in
    // from each corner rather than struts across the glass.
    let brace = (h / 4).clamp(2, 6);
    for step in 0..brace {
        canvas.plot(1 + step, 1 + step, 0.02, '╲', style);
        canvas.plot(w - 2 - step, 1 + step, 0.02, '╱', style);
        canvas.plot(1 + step, h - 2 - step, 0.02, '╱', style);
        canvas.plot(w - 2 - step, h - 2 - step, 0.02, '╲', style);
    }
    // The coaming across the very bottom of the glass.
    for x in 0..w {
        canvas.plot(x, h - 2, 0.02, '▄', style);
    }
}

/// A world hanging in the sky: a disc, shaded across its face.
fn draw_planet_disc(
    canvas: &mut Canvas,
    centre: (i16, i16),
    radius: i16,
    shade: char,
    style: zmax_view::graphics::Style,
) {
    for dy in -radius..=radius {
        for dx in -(radius * 2)..=(radius * 2) {
            // Cells are twice as tall as wide, so the disc is drawn wide.
            let inside = (dx * dx) as f32 / 4.0 + (dy * dy) as f32 <= (radius * radius) as f32;
            if !inside {
                continue;
            }
            let limb = ((dx * dx) as f32 / 4.0 + (dy * dy) as f32).sqrt() / radius as f32;
            let glyph = if limb > 0.86 { '░' } else { shade };
            canvas.plot(centre.1 + dx, centre.0 + dy, 800.0, glyph, style);
        }
    }
}

/// A flat grid running away from the camera, which is what gives a floor its
/// perspective.
fn draw_ground(
    canvas: &mut Canvas,
    cam: &Camera,
    height: f32,
    extent: f32,
    step: f32,
    glyph: char,
    style: zmax_view::graphics::Style,
) {
    let (cx, cz) = (cam.x, cam.z);
    let mut z = cz - extent;
    while z <= cz + extent {
        let mut x = cx - extent;
        while x <= cx + extent {
            if let Some((sx, sy, depth)) = cam.project(x, height, z) {
                canvas.plot(sx as i16, sy as i16, depth + 0.4, glyph, style);
            }
            x += step;
        }
        z += step;
    }
}

/// The camera a walking pilot sees through: eye height, looking the way they
/// are facing.
fn walker_camera(deck: &Deck, w: i16, h: i16) -> Camera {
    let (dr, dc) = deck.facing;
    Camera {
        x: deck.pilot.1 as f32,
        y: 1.1,
        z: deck.pilot.0 as f32,
        yaw: (dc as f32).atan2(dr as f32),
        focal: (w as f32) * 0.55,
        w: w as f32,
        h: h as f32,
    }
}

/// The camera looking out of a fighter's canopy, up the court.
fn pilot_camera(ship: (i16, i16), w: i16, h: i16) -> Camera {
    Camera {
        x: ship.1 as f32,
        y: 0.8,
        z: ship.0 as f32,
        yaw: std::f32::consts::PI,
        focal: (w as f32) * 0.7,
        w: w as f32,
        h: h as f32,
    }
}

/// The fighters, built out of boxes: fuselage, wings, engines and guns.
static AWING_HULL: [Solid; 3] = [
    Solid::new([0.0, 0.0, 0.0], [0.35, 0.3, 1.1], '█'),
    Solid::new([-0.7, 0.0, -0.5], [0.35, 0.15, 0.5], '▓'),
    Solid::new([0.7, 0.0, -0.5], [0.35, 0.15, 0.5], '▓'),
];
static XWING_HULL: [Solid; 5] = [
    Solid::new([0.0, 0.0, 0.2], [0.3, 0.3, 1.3], '█'),
    Solid::new([-1.0, 0.45, -0.4], [0.9, 0.08, 0.45], '▓'),
    Solid::new([1.0, 0.45, -0.4], [0.9, 0.08, 0.45], '▓'),
    Solid::new([-1.0, -0.45, -0.4], [0.9, 0.08, 0.45], '▓'),
    Solid::new([1.0, -0.45, -0.4], [0.9, 0.08, 0.45], '▓'),
];
static YWING_HULL: [Solid; 4] = [
    Solid::new([0.0, 0.0, 0.1], [0.3, 0.3, 1.2], '█'),
    Solid::new([-0.9, 0.1, -0.2], [0.3, 0.3, 0.9], '▓'),
    Solid::new([0.9, 0.1, -0.2], [0.3, 0.3, 0.9], '▓'),
    Solid::new([0.0, 0.0, -1.1], [1.1, 0.1, 0.2], '▒'),
];
static BWING_HULL: [Solid; 3] = [
    Solid::new([0.0, 0.0, 0.0], [0.28, 1.4, 0.9], '█'),
    Solid::new([-0.9, -0.6, -0.2], [0.7, 0.1, 0.4], '▓'),
    Solid::new([0.9, 0.6, -0.2], [0.7, 0.1, 0.4], '▓'),
];
static FREIGHTER_HULL: [Solid; 3] = [
    Solid::new([0.0, 0.0, 0.0], [1.3, 0.35, 1.3], '█'),
    Solid::new([0.7, 0.0, 1.0], [0.4, 0.2, 0.7], '▓'),
    Solid::new([-0.5, 0.45, 0.2], [0.35, 0.25, 0.35], '▒'),
];

/// The Imperial hulls: ball and panels, or a body and wings.
static TIE_HULL: [Solid; 3] = [
    Solid::new([0.0, 0.0, 0.0], [0.45, 0.45, 0.45], '█'),
    Solid::new([-1.0, 0.0, 0.0], [0.12, 1.0, 0.9], '▓'),
    Solid::new([1.0, 0.0, 0.0], [0.12, 1.0, 0.9], '▓'),
];
static INTERCEPTOR_HULL: [Solid; 3] = [
    Solid::new([0.0, 0.0, 0.0], [0.4, 0.4, 0.5], '█'),
    Solid::new([-0.9, 0.2, 0.0], [0.1, 1.1, 0.5], '▓'),
    Solid::new([0.9, 0.2, 0.0], [0.1, 1.1, 0.5], '▓'),
];
static BOMBER_HULL: [Solid; 4] = [
    Solid::new([-0.35, 0.0, 0.0], [0.4, 0.4, 0.8], '█'),
    Solid::new([0.35, 0.0, 0.0], [0.4, 0.4, 0.8], '█'),
    Solid::new([-1.1, 0.0, 0.0], [0.1, 0.9, 0.7], '▓'),
    Solid::new([1.1, 0.0, 0.0], [0.1, 0.9, 0.7], '▓'),
];
static GUNBOAT_HULL: [Solid; 3] = [
    Solid::new([0.0, 0.0, 0.0], [0.6, 0.5, 1.2], '█'),
    Solid::new([-0.9, 0.0, -0.3], [0.5, 0.15, 0.6], '▓'),
    Solid::new([0.9, 0.0, -0.3], [0.5, 0.15, 0.6], '▓'),
];
static PLATFORM_HULL: [Solid; 2] = [
    Solid::new([0.0, 0.0, 0.0], [0.8, 0.8, 0.8], '█'),
    Solid::new([0.0, 0.8, 0.0], [0.2, 0.5, 0.2], '▓'),
];
static PLAIN_HULL: [Solid; 1] = [Solid::new([0.0, 0.0, 0.0], [0.5, 0.5, 0.5], '█')];

impl ShipClass {
    /// The fighter as a solid, for the views that draw it in three dimensions.
    pub fn solid(self) -> &'static [Solid] {
        match self {
            ShipClass::AWing => &AWING_HULL,
            ShipClass::XWing => &XWING_HULL,
            ShipClass::YWing => &YWING_HULL,
            ShipClass::BWing => &BWING_HULL,
            ShipClass::Freighter => &FREIGHTER_HULL,
        }
    }
}

impl EnemyKind {
    /// The Imperial hull as a solid.
    pub fn solid(self) -> &'static [Solid] {
        match self {
            EnemyKind::TieFighter | EnemyKind::TieAdvanced => &TIE_HULL,
            EnemyKind::TieInterceptor | EnemyKind::TieDefender => &INTERCEPTOR_HULL,
            EnemyKind::TieBomber => &BOMBER_HULL,
            EnemyKind::Gunboat => &GUNBOAT_HULL,
            EnemyKind::GunPlatform => &PLATFORM_HULL,
            _ => &PLAIN_HULL,
        }
    }
}

/// Which way the fight is watched.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    /// Straight down on the court, the way the arcade did it.
    TopDown,
    /// Out of the canopy, with everything ahead projected onto the glass.
    Cockpit,
}

impl ViewMode {
    pub fn name(self) -> &'static str {
        match self {
            ViewMode::TopDown => "top-down",
            ViewMode::Cockpit => "cockpit",
        }
    }
}

/// The shading ramp: near hull is solid, far hull fades out.
fn depth_shade(depth: i16) -> &'static str {
    match depth {
        0..=3 => "█",
        4..=7 => "▓",
        8..=13 => "▒",
        _ => "░",
    }
}

/// How far ahead the canopy sees: the whole court, so nothing in front of the
/// hull is ever off the glass.
const COCKPIT_DEPTH: i16 = H;

/// The interactive Nova overlay.
pub struct Nova {
    game: Game,
    seed: u64,
    /// Highlighted hull on the picker.
    pick: usize,
    /// Highlighted difficulty on the picker.
    diff: usize,
    /// Highlighted galaxy on the picker.
    gal: usize,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
    /// Rendered frames, used only to blink the hull while it is invulnerable.
    frames: u64,
    /// True while the squadron roster is up over everything else.
    roster: bool,
    /// Whether the fight is watched from above or out of the canopy.
    view: ViewMode,
}

impl Nova {
    pub fn new() -> Self {
        Nova {
            game: Game::new(1),
            seed: 1,
            pick: 1,
            diff: 0,
            gal: 0,
            paused: false,
            last: None,
            interval: Duration::from_millis(70),
            frames: 0,
            roster: false,
            view: ViewMode::TopDown,
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
        let (class, difficulty, galaxy) = (self.game.class, self.game.difficulty, self.game.galaxy);
        self.restart();
        self.game.start(class, difficulty, galaxy);
    }

    /// Launch the highlighted hull at the highlighted difficulty.
    fn launch(&mut self) {
        let class = ShipClass::ALL[self.pick];
        let difficulty = Difficulty::ALL[self.diff];
        let galaxy = Galaxy::ALL[self.gal];
        self.game.start(class, difficulty, galaxy);
    }

    /// Running = a live round or the cleared-wave pause, not paused, and not
    /// sitting with the roster up.
    fn running(&self) -> bool {
        matches!(
            self.game.status,
            Status::Playing | Status::WaveClear | Status::Surface
        ) && !self.paused
            && !self.roster
    }

    /// The fight as it looks out of the canopy: everything ahead of the hull
    /// projected onto the glass, near contacts big and low, far ones small and
    /// close to the horizon.
    fn render_cockpit(&self, area: Rect, surface: &mut Surface, ctx: &Context) {
        let theme = &ctx.editor.theme;
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let frame_style = theme.get("ui.linenr");
        let enemy_style = theme.get("error");
        let boss_style = theme.get("error");
        let shot_style = theme.get("warning");
        let star_style = theme.get("ui.linenr");
        let g = &self.game;
        let ox = area.x + 1;
        let oy = area.y + 1;
        let glass_w = (area.width as i16 - 4).clamp(20, 96);
        let glass_h = (area.height as i16 - 8).clamp(8, 30);
        let centre_x = glass_w / 2;
        let horizon = glass_h / 3;
        let put = |surface: &mut Surface, x: i16, y: i16, glyph: &str, style| {
            if (0..glass_w).contains(&x) && (0..glass_h).contains(&y) {
                surface.set_string(ox + x as u16, oy + y as u16, glyph, style);
            }
        };
        // Project a court cell onto the glass. The whole width of the court maps
        // onto the glass, so every contact ahead is somewhere on it; near rows
        // spread a little wider than far ones, and anything that would fall off
        // the edge is held at the edge rather than dropped.
        let project = |row: i16, col: i16| -> Option<(i16, i16, i16)> {
            let depth = g.ship.0 - row;
            if !(0..COCKPIT_DEPTH).contains(&depth) {
                return None;
            }
            let near = COCKPIT_DEPTH - depth;
            let dx = col - g.ship.1;
            let spread = (glass_w * (COCKPIT_DEPTH + near)) / (W * COCKPIT_DEPTH);
            let x = (centre_x + dx * spread.max(1)).clamp(1, glass_w - 2);
            let y = horizon + (near * (glass_h - horizon - 3)) / COCKPIT_DEPTH;
            Some((x, y, depth))
        };

        // Everything ahead, drawn as solid geometry into a depth buffer: the
        // nearest hull wins each cell, so ships pass in front of each other.
        let mut canvas = Canvas::new(glass_w, glass_h);
        let cam = pilot_camera(g.ship, glass_w, glass_h);
        // Space first: a black field, the world this system belongs to hanging
        // in it, and the sector's own haze.
        draw_horizon(&mut canvas, &cam, '·', '·', star_style, star_style);
        if g.planet != Planet::DeepSpace {
            draw_planet_disc(
                &mut canvas,
                (glass_h / 4, glass_w / 4),
                (glass_h / 5).max(3),
                g.planet.shade().chars().next().unwrap_or('▓'),
                frame_style,
            );
        }
        for star in &g.stars {
            let y = ((star.pos.1 % 7) as f32 - 3.0) * 0.8;
            if let Some((sx, sy, depth)) = cam.project(star.pos.1 as f32, y, star.pos.0 as f32) {
                canvas.plot(
                    sx as i16,
                    sy as i16,
                    depth + 30.0,
                    if depth < 8.0 { '*' } else { '·' },
                    star_style,
                );
            }
        }
        for e in &g.enemies {
            if e.pos.0 > g.ship.0 {
                continue;
            }
            draw_model(
                &mut canvas,
                &cam,
                e.kind.solid(),
                [e.pos.1 as f32, 0.0, e.pos.0 as f32],
                HULL_SCALE,
                enemy_style,
            );
        }
        for (index, (r, c)) in g.wing_cells() {
            draw_model(
                &mut canvas,
                &cam,
                g.squad[index].class.solid(),
                [c as f32, -0.6, r as f32],
                HULL_SCALE,
                header_style,
            );
        }
        if let Some(cap) = &g.capital {
            // The capital as stacked plating: one slab per row of hull.
            for dr in 0..cap.kind.depth() {
                let span = cap.kind.span(dr) as f32;
                let slab = Solid::new([0.0, 0.0, 0.0], [span, 0.6, 0.5], '█');
                draw_solid(
                    &mut canvas,
                    &cam,
                    &slab,
                    [cap.pos.1 as f32, 1.5, (cap.pos.0 + dr) as f32],
                    1.0,
                    boss_style,
                );
            }
            for part in &cap.parts {
                if part.hp <= 0 {
                    continue;
                }
                let (r, c) = cap.part_cell(part);
                let tower = Solid::new([0.0, 0.0, 0.0], [0.6, 0.8, 0.6], '▓');
                draw_solid(
                    &mut canvas,
                    &cam,
                    &tower,
                    [c as f32, 2.6, r as f32],
                    1.0,
                    header_style,
                );
            }
        }
        if let Some(ally) = &g.ally {
            for dr in 0..ally.kind.depth() {
                let span = ally.kind.span(dr) as f32;
                let slab = Solid::new([0.0, 0.0, 0.0], [span, 0.5, 0.5], '▓');
                draw_solid(
                    &mut canvas,
                    &cam,
                    &slab,
                    [ally.pos.1 as f32, -1.6, (ally.pos.0 + dr) as f32],
                    1.0,
                    header_style,
                );
            }
        }
        for s in &g.enemy_shots {
            if let Some((sx, sy, depth)) = cam.project(s.pos.1 as f32, 0.0, s.pos.0 as f32) {
                canvas.plot(sx as i16, sy as i16, depth, '!', enemy_style);
            }
        }
        for s in &g.shots {
            if let Some((sx, sy, depth)) = cam.project(s.pos.1 as f32, 0.0, s.pos.0 as f32) {
                canvas.plot(sx as i16, sy as i16, depth, '|', shot_style);
            }
        }
        canvas.blit(surface, ox, oy);
        // The capital ship: a wall of hull hanging over the horizon.
        if let Some(cap) = &g.capital {
            for dr in 0..cap.kind.depth() {
                let span = cap.kind.span(dr);
                for dc in -span..=span {
                    if let Some((x, y, depth)) = project(cap.pos.0 + dr, cap.pos.1 + dc) {
                        // Plating shaded by how far off the hull is, so the
                        // ship reads as a solid mass rather than a line.
                        put(surface, x, y, depth_shade(depth), boss_style);
                    }
                }
            }
            for part in &cap.parts {
                if part.hp <= 0 {
                    continue;
                }
                let (r, c) = cap.part_cell(part);
                if let Some((x, y, _)) = project(r, c) {
                    put(surface, x, y, part.kind.glyph(), header_style);
                }
            }
        }
        draw_canopy(&mut canvas, frame_style);
        canvas.blit(surface, ox, oy);

        // What is behind you goes on the rear-view strip rather than the glass.
        let behind = g.enemies.iter().filter(|e| e.pos.0 > g.ship.0).count();
        // Fire, ours and theirs.
        for s in &g.enemy_shots {
            if let Some((x, y, _)) = project(s.pos.0, s.pos.1) {
                put(surface, x, y, "!", enemy_style);
            }
        }
        for s in &g.shots {
            if let Some((x, y, _)) = project(s.pos.0, s.pos.1) {
                put(surface, x, y, "|", shot_style);
            }
        }
        // The scope, bottom right: the whole court from above, so nothing on
        // the board is ever a surprise.
        let scope_w = 22i16;
        let scope_h = 9i16;
        let sx0 = glass_w - scope_w - 2;
        let sy0 = glass_h - scope_h - 1;
        if sx0 > 2 {
            for y in 0..scope_h {
                for x in 0..scope_w {
                    let edge = x == 0 || y == 0 || x == scope_w - 1 || y == scope_h - 1;
                    if edge {
                        put(surface, sx0 + x, sy0 + y, "·", frame_style);
                    }
                }
            }
            let plot = |surface: &mut Surface, row: i16, col: i16, glyph: &str, style| {
                let x = sx0 + 1 + col * (scope_w - 2) / W;
                let y = sy0 + 1 + row * (scope_h - 2) / H;
                if (sx0 + 1..sx0 + scope_w - 1).contains(&x)
                    && (sy0 + 1..sy0 + scope_h - 1).contains(&y)
                {
                    surface.set_string(ox + x as u16, oy + y as u16, glyph, style);
                }
            };
            for e in &g.enemies {
                plot(surface, e.pos.0, e.pos.1, "•", enemy_style);
            }
            if let Some(cap) = &g.capital {
                for dc in (-cap.kind.span(0)..=cap.kind.span(0)).step_by(3) {
                    plot(surface, cap.pos.0, cap.pos.1 + dc, "▬", enemy_style);
                }
            }
            for (_, (r, c)) in g.wing_cells() {
                plot(surface, r, c, "+", header_style);
            }
            plot(surface, g.ship.0, g.ship.1, "▲", theme.get("function"));
        }
        // Rear view: what is on your tail, and how much of it.
        if behind > 0 {
            let rear = format!("◄ REAR {behind} ►");
            let rx = (centre_x - rear.chars().count() as i16 / 2).max(1);
            surface.set_string(ox + rx as u16, oy + 1, &rear, enemy_style);
        }
        // The wing, off the glass either side: they fly beside the hull, not
        // ahead of it, so they are drawn low and wide where a pilot would
        // actually see them.
        for (index, (r, c)) in g.wing_cells() {
            let wing = &g.squad[index];
            let dx = c - g.ship.1;
            let x = centre_x + dx * 2;
            let y = (glass_h - 5 + (r - g.ship.0).clamp(0, 2)).min(glass_h - 2);
            for (i, glyph) in wing.class.sprite()[1].chars().enumerate() {
                if glyph != ' ' {
                    put(
                        surface,
                        x + i as i16 - 1,
                        y,
                        &glyph.to_string(),
                        header_style,
                    );
                }
            }
            let label = format!("{} {}", wing.name, "▮".repeat(wing.shield as usize));
            let lx = (x - 2).clamp(1, glass_w - label.chars().count() as i16 - 2);
            surface.set_string(
                ox + lx as u16,
                oy + (y + 1).min(glass_h - 1) as u16,
                &label,
                frame_style,
            );
        }
        // The drones ride the wingtips.
        for &side in &g.drones {
            let x = centre_x + side * 8;
            put(surface, x, glass_h - 3, "◇", header_style);
        }
        // The nose of your own fighter, filling the bottom of the glass.
        for (i, glyph) in g.class.sprite()[1].chars().enumerate() {
            if glyph != ' ' {
                put(
                    surface,
                    centre_x + i as i16 - 1,
                    glass_h - 2,
                    &glyph.to_string(),
                    theme.get("function"),
                );
            }
        }
        // Cannon tracers converging on the reticle from the wing roots.
        for i in 1..4 {
            let y = glass_h - 1 - i;
            put(surface, centre_x - 6 + i, y, "╲", shot_style);
            put(surface, centre_x + 6 - i, y, "╱", shot_style);
        }
        // The reticle, and a box on whatever is dead ahead.
        let target = g
            .enemies
            .iter()
            .filter(|e| e.pos.0 < g.ship.0 && (e.pos.1 - g.ship.1).abs() <= 3)
            .min_by_key(|e| g.ship.0 - e.pos.0);
        if let Some(e) = target {
            if let Some((x, y, _)) = project(e.pos.0, e.pos.1) {
                put(surface, x - 2, y, "⟦", header_style);
                put(surface, x + 2, y, "⟧", header_style);
            }
        }
        put(surface, centre_x, horizon + 1, "┼", header_style);
        put(surface, centre_x - 2, horizon + 1, "─", frame_style);
        put(surface, centre_x + 2, horizon + 1, "─", frame_style);
        // Canopy frame: struts down the sides and the dash across the bottom.
        for y in 0..glass_h {
            put(surface, 0, y, "║", frame_style);
            put(surface, glass_w - 1, y, "║", frame_style);
        }
        for x in 0..glass_w {
            put(surface, x, 0, "═", frame_style);
            put(surface, x, glass_h - 1, "▁", frame_style);
        }
        let dash = oy + glass_h as u16;
        surface.set_string(
            ox,
            dash,
            &format!(
                "SHIELDS {}{}   LASERS {}   ENGINES {}   FORCE {}   TORPEDOES {}   CONTACTS {}",
                "▮".repeat(g.shield as usize),
                "▯".repeat(g.max_shield.saturating_sub(g.shield) as usize),
                "▮".repeat(g.power.lasers as usize),
                "▮".repeat(g.power.engines as usize),
                "▰".repeat((g.force * 6 / FORCE_MAX) as usize),
                g.missiles,
                g.enemies.len() + g.capital.iter().count() + g.boss.iter().count()
            ),
            text_style,
        );
        surface.set_string(
            ox,
            dash + 1,
            &format!(
                "{} · {} over {} · {}",
                g.class.name(),
                g.sector.name(),
                g.planet.name(),
                g.objective.label()
            ),
            header_style,
        );
        if let Some(line) = g.chatter.first() {
            surface.set_string(ox, dash + 2, &line.line, header_style);
        }
        surface.set_string(
            ox,
            dash + 3,
            "o returns to top-down · p pause · t roster · SPC fire · m torpedoes",
            frame_style,
        );
    }

    /// The ceremony: the war is over and the squadron is decorated for it.
    fn render_ceremony(&self, area: Rect, surface: &mut Surface, ctx: &Context) {
        let theme = &ctx.editor.theme;
        let header = theme.get("ui.text.focus");
        let text = theme.get("ui.text");
        let dim = theme.get("ui.linenr");
        let g = &self.game;
        let ox = area.x + 4;
        let mut y = area.y + 2;
        surface.set_string(ox, y, "THE WAR IS OVER", header);
        y += 2;
        surface.set_string(
            ox,
            y,
            &format!(
                "{} {} — {} · {} kills logged · {} medals",
                g.rank().name(),
                g.class.name(),
                g.galaxy.name(),
                g.score / 100,
                g.medals
            ),
            text,
        );
        y += 2;
        surface.set_string(ox, y, "THE SQUADRON", header);
        y += 1;
        for wing in &g.squad {
            surface.set_string(
                ox,
                y,
                &format!(
                    "  {:<12} {:<20} {:<16} {}",
                    wing.name,
                    wing.class.name(),
                    wing.weapon.name(),
                    if wing.alive {
                        "flew it out"
                    } else {
                        "did not come back"
                    }
                ),
                if wing.alive { text } else { dim },
            );
            y += 1;
        }
        y += 1;
        surface.set_string(ox, y, "MISSIONS FLOWN", header);
        y += 1;
        for mission in Mission::CAMPAIGN.iter() {
            surface.set_string(ox, y, &format!("  {}", mission.name), dim);
            y += 1;
        }
        y += 1;
        surface.set_string(
            ox,
            y,
            &format!(
                "Final score {}   pilot level {}   salvage banked {}",
                g.score, g.level, g.credits
            ),
            header,
        );
        surface.set_string(ox, y + 2, "n flies another war · q stands down", text);
    }

    /// The pause panel: what the mission wants, what is in the racks, and every
    /// key that changes the loadout without flying.
    fn render_pause(&self, area: Rect, surface: &mut Surface, ctx: &Context) {
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
                "PAUSED — {} · {}",
                g.mission.map_or("free flight", |m| m.name),
                g.objective.label()
            ),
            header,
        );
        y += 1;
        surface.set_string(
            ox,
            y,
            &format!(
                "{} over {}   {}",
                g.sector.name(),
                g.planet.name(),
                g.planet.blurb()
            ),
            dim,
        );
        y += 2;
        surface.set_string(ox, y, "GUNS — press the number to fit it", header);
        y += 1;
        for (i, weapon) in g.owned.iter().enumerate() {
            let fitted = *weapon == g.weapon;
            surface.set_string(
                ox,
                y,
                &format!(
                    "{} [{}] {:<20} L{}",
                    if fitted { "▶" } else { " " },
                    (i + 1) % 10,
                    weapon.name(),
                    if fitted { g.weapon_level } else { 1 }
                ),
                if fitted { header } else { text },
            );
            y += 1;
        }
        y += 1;
        surface.set_string(
            ox,
            y,
            &format!(
                "POWER   [z] lasers {}   [c] deflectors {}   [v] engines {}",
                "▮".repeat(g.power.lasers as usize),
                "▮".repeat(g.power.shields as usize),
                "▮".repeat(g.power.engines as usize)
            ),
            text,
        );
        y += 1;
        surface.set_string(
            ox,
            y,
            &format!(
                "FORCE   {}/{}   [e] sense ({})   [y] pull ({})   [u] guided ({})",
                g.force, FORCE_MAX, SENSE_COST, PULL_COST, GUIDED_COST
            ),
            text,
        );
        y += 1;
        surface.set_string(
            ox,
            y,
            &format!(
                "STORES  torpedoes {}   bombs {}   shields {}/{}   lives {}",
                g.missiles, g.bombs, g.shield, g.max_shield, g.lives
            ),
            text,
        );
        y += 1;
        surface.set_string(
            ox,
            y,
            &format!(
                "WING    {} of {} flying · {} — {}   [g] changes it",
                g.squad.iter().filter(|w| w.alive).count(),
                g.squad.len(),
                g.wing_formation.name(),
                g.wing_formation.blurb()
            ),
            text,
        );
        y += 2;
        surface.set_string(
            ox,
            y,
            "p resumes · o swaps view · g formation · t roster · [ ] guns · r retry · n new",
            dim,
        );
    }

    /// The squadron roster: every fighter, what it is carrying and how it is
    /// holding up. It opens over whatever is on screen, in flight or not.
    fn render_roster(&self, area: Rect, surface: &mut Surface, ctx: &Context) {
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
                "SQUADRON ROSTER — {} · {}   salvage {}   score {}",
                g.galaxy.name(),
                g.difficulty.name(),
                g.credits,
                g.score
            ),
            header,
        );
        y += 1;
        surface.set_string(
            ox,
            y,
            &format!(
                "pilot level {} ({}/{} xp)   lives {}   torpedoes {}   bombs {}",
                g.level, g.xp, g.xp_next, g.lives, g.missiles, g.bombs
            ),
            text,
        );
        y += 2;
        surface.set_string(
            ox,
            y,
            "  callsign     fighter              gun               shield  E R P C M  modules",
            dim,
        );
        y += 1;
        for (i, wing) in g.squad.iter().enumerate() {
            let flown = i == g.active;
            let style = if !wing.alive {
                dim
            } else if flown {
                header
            } else {
                text
            };
            let modules: Vec<&str> = wing.loadout.modules.iter().map(|m| m.name()).collect();
            surface.set_string(
                ox,
                y,
                &format!(
                    "{} {:<12} {:<20} {:<14} L{}  {}/{}    {} {} {} {} {}  {}",
                    if flown { "▶" } else { " " },
                    wing.name,
                    wing.class.name(),
                    wing.weapon.name(),
                    wing.weapon_level,
                    if flown { g.shield } else { wing.shield },
                    if flown { g.max_shield } else { wing.max_shield },
                    wing.loadout.tier(Part::Engine),
                    wing.loadout.tier(Part::Reactor),
                    wing.loadout.tier(Part::Plating),
                    wing.loadout.tier(Part::Cannon),
                    wing.loadout.tier(Part::Magazine),
                    if modules.is_empty() {
                        wing.status().to_string()
                    } else {
                        modules.join(", ")
                    }
                ),
                style,
            );
            y += 1;
        }
        y += 1;
        surface.set_string(
            ox,
            y,
            &format!(
                "RACKS   {}",
                g.owned
                    .iter()
                    .enumerate()
                    .map(|(i, w)| format!("{}:{}", i + 1, w.name()))
                    .collect::<Vec<_>>()
                    .join("   ")
            ),
            text,
        );
        y += 1;
        surface.set_string(
            ox,
            y,
            &format!(
                "WING    {} — {}",
                g.wing_formation.name(),
                g.wing_formation.blurb()
            ),
            text,
        );
        y += 1;
        surface.set_string(
            ox,
            y,
            &format!(
                "POWER   lasers {}  shields {}  engines {}   (z / c / v in flight)",
                "▮".repeat(g.power.lasers as usize),
                "▮".repeat(g.power.shields as usize),
                "▮".repeat(g.power.engines as usize)
            ),
            text,
        );
        y += 2;
        surface.set_string(
            ox,
            y,
            "t closes the roster · w climbs into the next fighter (in the hangar) · q quits",
            dim,
        );
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
            "Pick a fighter — ←/→ or 1-5 · d difficulty · g galaxy · Enter free flight · m campaign",
            text,
        );
        y += 2;
        for (i, class) in ShipClass::ALL.iter().enumerate() {
            let marker = if i == self.pick { "▶" } else { " " };
            let style = if i == self.pick { header } else { text };
            surface.set_string(
                ox,
                y,
                &format!("{marker} {}  {}", i + 1, class.name()),
                style,
            );
            let sprite = class.sprite();
            surface.set_string(ox + 17, y, sprite[0], style);
            surface.set_string(ox + 17, y + 1, sprite[1], style);
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
                "Difficulty  [{}]   +{} armour on every enemy, ×{} score   (d cycles)",
                difficulty.name(),
                difficulty.armour(),
                difficulty.score_bonus()
            ),
            header,
        );
        y += 1;
        let galaxy = Galaxy::ALL[self.gal];
        surface.set_string(
            ox,
            y,
            &format!(
                "Galaxy      [{}]   {}   (g cycles)",
                galaxy.name(),
                galaxy.blurb()
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
            "m flies the campaign: Tatooine, the Kessel Run, Yavin, Hoth, Cloud City, Endor.",
            dim,
        );
    }

    /// The hangar deck from above: fighters in their bays along the back wall,
    /// the stations down the front, and the pilot walking between them.
    fn render_deck(&self, area: Rect, surface: &mut Surface, ctx: &Context) {
        let theme = &ctx.editor.theme;
        let header = theme.get("ui.text.focus");
        let text = theme.get("ui.text");
        let dim = theme.get("ui.linenr");
        let ship_style = theme.get("function");
        let g = &self.game;
        let deck = &g.deck;
        let ox = area.x + 2;
        let oy = area.y + 3;
        surface.set_string(
            ox,
            area.y,
            &format!(
                "{} — {} · {}   salvage {}",
                if g.status == Status::Surface {
                    format!("{} SURFACE", g.planet.name().to_uppercase())
                } else {
                    "HANGAR DECK".to_string()
                },
                g.galaxy.name(),
                g.rank().name(),
                g.credits
            ),
            header,
        );
        surface.set_string(
            ox,
            area.y + 1,
            "walk ←/→/↑/↓ · Enter uses it · w fighter · f formation · o first person · t roster",
            dim,
        );
        // The deck is bigger than any window, so the view follows the pilot.
        let view_w = (area.width as i16 - 4).clamp(20, deck.width);
        let view_h = (area.height as i16 - 8).clamp(8, deck.height);
        let cam_c = (deck.pilot.1 - view_w / 2).clamp(0, deck.width - view_w);
        let cam_r = (deck.pilot.0 - view_h / 2).clamp(0, deck.height - view_h);
        let on_view = |r: i16, c: i16| {
            (cam_r..cam_r + view_h).contains(&r) && (cam_c..cam_c + view_w).contains(&c)
        };
        let at = |r: i16, c: i16| ((c - cam_c) as u16, (r - cam_r) as u16);
        for r in cam_r..cam_r + view_h {
            for c in cam_c..cam_c + view_w {
                let edge = r == 0 || r == deck.height - 1 || c == 0 || c == deck.width - 1;
                let (vx, vy) = at(r, c);
                // Deck plating in a hangar; the ground itself on a world.
                let glyph = if edge {
                    "▒"
                } else if g.status == Status::Surface {
                    if (r * 7 + c * 3) % 11 == 0 {
                        g.planet.shade()
                    } else {
                        " "
                    }
                } else if r % 4 == 0 && c % 6 == 0 {
                    "·"
                } else {
                    " "
                };
                surface.set_string(ox + vx, oy + vy, glyph, dim);
            }
        }
        // Cover: blocks of city, rock or trees.
        for block in &deck.cover {
            for r in block.pos.0..block.pos.0 + block.size.0 {
                for c in block.pos.1..block.pos.1 + block.size.1 {
                    if on_view(r, c) {
                        let (vx, vy) = at(r, c);
                        surface.set_string(
                            ox + vx,
                            oy + vy,
                            if block.tall { "▓" } else { "▒" },
                            text,
                        );
                    }
                }
            }
        }
        for trooper in &deck.troopers {
            if on_view(trooper.pos.0, trooper.pos.1) {
                let (vx, vy) = at(trooper.pos.0, trooper.pos.1);
                surface.set_string(
                    ox + vx,
                    oy + vy,
                    trooper.kind.glyph(),
                    if trooper.kind == GroundKind::Duellist {
                        theme.get("warning")
                    } else {
                        theme.get("error")
                    },
                );
            }
        }
        for bolt in &deck.bolts {
            if on_view(bolt.pos.0, bolt.pos.1) {
                let (vx, vy) = at(bolt.pos.0, bolt.pos.1);
                surface.set_string(
                    ox + vx,
                    oy + vy,
                    if bolt.friendly { "-" } else { "=" },
                    if bolt.friendly {
                        header
                    } else {
                        theme.get("error")
                    },
                );
            }
        }
        // Everything worth walking up to.
        for (spot, pos) in &deck.spots {
            let style = if Some(*spot) == deck.at_hand() {
                header
            } else {
                text
            };
            match spot {
                DeckSpot::Bay(index) => {
                    let wing = g.squad.get(*index);
                    let sprite = wing.map_or(["   ", "   "], |w| w.class.sprite());
                    for (row, line) in sprite.iter().enumerate() {
                        for (i, glyph) in line.chars().enumerate() {
                            let (r, c) = (pos.0 + row as i16, pos.1 + i as i16 - 1);
                            if glyph != ' ' && on_view(r, c) {
                                let (vx, vy) = at(r, c);
                                surface.set_string(
                                    ox + vx,
                                    oy + vy,
                                    &glyph.to_string(),
                                    if *index == g.active {
                                        ship_style
                                    } else {
                                        style
                                    },
                                );
                            }
                        }
                    }
                    if let Some(wing) = wing.filter(|_| on_view(pos.0 + 2, pos.1)) {
                        let (vx, vy) = at(pos.0 + 2, (pos.1 - 2).max(0));
                        surface.set_string(
                            ox + vx,
                            oy + vy,
                            &format!("{} {}", wing.name, if wing.alive { "" } else { "(down)" }),
                            if *index == g.active { header } else { dim },
                        );
                    }
                }
                _ => {
                    surface.set_string(ox + pos.1 as u16, oy + pos.0 as u16, spot.glyph(), style);
                    surface.set_string(
                        ox + (pos.1 - 3).max(0) as u16,
                        oy + (pos.0 + 1) as u16,
                        spot.name(),
                        dim,
                    );
                }
            }
        }
        // The pilot.
        let (px, py) = at(deck.pilot.0, deck.pilot.1);
        surface.set_string(ox + px, oy + py, "Å", header);
        let foot = oy + view_h as u16 + 1;
        let standing = match deck.at_hand() {
            Some(DeckSpot::Bay(i)) => {
                let wing = &g.squad[i];
                format!(
                    "{} — {} · {} L{} · shields {}/{} · {}",
                    wing.name,
                    wing.class.name(),
                    wing.weapon.name(),
                    wing.weapon_level,
                    wing.shield,
                    wing.max_shield,
                    wing.status()
                )
            }
            Some(spot) => format!("{} — press Enter", spot.name()),
            None => "Walk up to a bay or a station.".to_string(),
        };
        surface.set_string(ox, foot, &standing, text);
        if g.status == Status::Surface {
            surface.set_string(
                ox,
                foot + 1,
                &format!(
                    "HEALTH {}{}   {}{}   hostiles {}   SPC strike · 1 belt · 2 speeder · b parry · e push",
                    "▮".repeat(deck.health.max(0) as usize),
                    "▯".repeat((PILOT_HEALTH - deck.health.max(0)) as usize),
                    deck.sidearm.name(),
                    if deck.riding { " · riding" } else { "" },
                    deck.troopers.len()
                ),
                text,
            );
        }
        if let Some(line) = g.chatter.first() {
            surface.set_string(ox, foot + 1, &line.line, header);
        }
    }

    /// The hangar from the deck plates: what the pilot is actually looking at.
    fn render_deck_pov(&self, area: Rect, surface: &mut Surface, ctx: &Context) {
        let theme = &ctx.editor.theme;
        let header = theme.get("ui.text.focus");
        let text = theme.get("ui.text");
        let dim = theme.get("ui.linenr");
        let ship_style = theme.get("function");
        let g = &self.game;
        let deck = &g.deck;
        let ox = area.x + 2;
        let oy = area.y + 3;
        let w = (area.width as i16 - 6).clamp(24, 120);
        let h = (area.height as i16 - 8).clamp(10, 40);
        let mut canvas = Canvas::new(w, h);
        let cam = walker_camera(deck, w, h);

        // Sky over ground, so there is a world before anything stands on it.
        let ground = if g.status == Status::Surface {
            g.planet.shade().chars().next().unwrap_or('·')
        } else {
            '·'
        };
        draw_horizon(
            &mut canvas,
            &cam,
            if g.status == Status::Surface {
                '░'
            } else {
                '▒'
            },
            ground,
            dim,
            dim,
        );
        if g.status == Status::Surface && g.planet != Planet::DeepSpace {
            // Something else in the sky: a moon, or the world you are orbiting.
            draw_planet_disc(
                &mut canvas,
                (h / 5, w * 3 / 4),
                (h / 8).max(2),
                g.planet.shade().chars().next().unwrap_or('▓'),
                text,
            );
        }
        draw_ground(&mut canvas, &cam, 0.0, 30.0, 2.0, ground, dim);
        // Lane markings running away from the pilot, which is what sells the
        // perspective on a flat floor.
        for lane in -6..=6 {
            let mut ahead = 1.0;
            while ahead < 26.0 {
                let (dr, dc) = deck.facing;
                let x = deck.pilot.1 as f32 + dc as f32 * ahead - dr as f32 * lane as f32;
                let z = deck.pilot.0 as f32 + dr as f32 * ahead + dc as f32 * lane as f32;
                if let Some((sx, sy, depth)) = cam.project(x, 0.02, z) {
                    canvas.plot(sx as i16, sy as i16, depth + 0.2, '·', dim);
                }
                ahead += 0.6;
            }
        }
        if g.status != Status::Surface {
            draw_ground(&mut canvas, &cam, 4.5, 20.0, 2.0, '╤', dim);
        }

        // Cover first: blocks of city or rock, drawn as solid volumes.
        for block in &deck.cover {
            let centre = [
                block.pos.1 as f32 + block.size.1 as f32 / 2.0,
                if block.tall { 1.6 } else { 0.4 },
                block.pos.0 as f32 + block.size.0 as f32 / 2.0,
            ];
            let solid = Solid::new(
                [0.0, 0.0, 0.0],
                [
                    block.size.1 as f32 / 2.0,
                    if block.tall { 2.2 } else { 0.7 },
                    block.size.0 as f32 / 2.0,
                ],
                '█',
            );
            draw_solid(&mut canvas, &cam, &solid, centre, 1.0, text);
        }
        for trooper in &deck.troopers {
            let body = Solid::new([0.0, 0.0, 0.0], [0.28, 0.75, 0.28], '█');
            draw_solid(
                &mut canvas,
                &cam,
                &body,
                [trooper.pos.1 as f32, 0.75, trooper.pos.0 as f32],
                1.0,
                theme.get("error"),
            );
        }
        for bolt in &deck.bolts {
            if let Some((sx, sy, depth)) = cam.project(bolt.pos.1 as f32, 0.9, bolt.pos.0 as f32) {
                canvas.plot(
                    sx as i16,
                    sy as i16,
                    depth,
                    if bolt.friendly { '-' } else { '=' },
                    if bolt.friendly {
                        header
                    } else {
                        theme.get("error")
                    },
                );
            }
        }
        // Everything standing on it, as solid geometry.
        for (spot, pos) in &deck.spots {
            let origin = [pos.1 as f32, 0.9, pos.0 as f32];
            match spot {
                DeckSpot::Bay(_) | DeckSpot::ParkedShip => {
                    let i = match spot {
                        DeckSpot::Bay(index) => *index,
                        _ => g.active,
                    };
                    let Some(wing) = g.squad.get(i) else {
                        continue;
                    };
                    let style = if i == g.active { ship_style } else { text };
                    draw_model(
                        &mut canvas,
                        &cam,
                        wing.class.solid(),
                        origin,
                        HULL_SCALE,
                        style,
                    );
                }
                _ => {
                    // A console: a lit slab with a screen on top of it.
                    let body = Solid::new([0.0, -0.3, 0.0], [1.1, 0.7, 1.1], '█');
                    let screen = Solid::new([0.0, 0.9, 0.0], [0.8, 0.4, 0.2], '▓');
                    let style = if Some(*spot) == deck.at_hand() {
                        header
                    } else {
                        text
                    };
                    draw_solid(&mut canvas, &cam, &body, origin, 1.0, style);
                    draw_solid(&mut canvas, &cam, &screen, origin, 1.0, style);
                }
            }
        }
        canvas.blit(surface, ox, oy);

        // Labels over the geometry, so you know what you are looking at.
        for (spot, pos) in &deck.spots {
            let label = match spot {
                DeckSpot::Bay(i) => match g.squad.get(*i) {
                    Some(wing) => format!("{} · {}", wing.name, wing.class.name()),
                    None => continue,
                },
                DeckSpot::ParkedShip => format!("{} · ready", g.class.name()),
                other => other.name().to_string(),
            };
            if let Some((sx, sy, depth)) = cam.project(pos.1 as f32, 2.0, pos.0 as f32) {
                if depth < 18.0 {
                    let x = (sx as i16 - label.chars().count() as i16 / 2).clamp(0, w - 1);
                    let y = (sy as i16).clamp(0, h - 1);
                    surface.set_string(
                        ox + x as u16,
                        oy + y as u16,
                        &label,
                        if Some(*spot) == deck.at_hand() {
                            header
                        } else {
                            dim
                        },
                    );
                }
            }
        }

        surface.set_string(
            ox,
            area.y,
            &format!(
                "{} — first person   facing {}",
                if g.status == Status::Surface {
                    g.planet.name().to_uppercase()
                } else {
                    "HANGAR DECK".to_string()
                },
                match deck.facing {
                    (-1, _) => "forward",
                    (1, _) => "aft",
                    (_, -1) => "port",
                    _ => "starboard",
                }
            ),
            header,
        );
        surface.set_string(
            ox,
            area.y + 1,
            "↑/↓ walk · ←/→ turn · SPC blaster · Enter uses what is ahead · o top-down",
            dim,
        );
        let foot = oy + h as u16;
        surface.set_string(
            ox,
            foot,
            &match deck.at_hand() {
                Some(DeckSpot::Bay(i)) => format!(
                    "In front of you: {} — {} · {} L{} · shields {}/{} · {}",
                    g.squad[i].name,
                    g.squad[i].class.name(),
                    g.squad[i].weapon.name(),
                    g.squad[i].weapon_level,
                    g.squad[i].shield,
                    g.squad[i].max_shield,
                    g.squad[i].status()
                ),
                Some(spot) => format!("In front of you: {} — press Enter", spot.name()),
                None => "Nothing in front of you. Walk on.".to_string(),
            },
            text,
        );
        if let Some(line) = g.chatter.first() {
            surface.set_string(ox, foot + 1, &line.line, header);
        }
    }

    /// The galaxy chart: every system, the lanes between them, and where the
    /// squad is parked.
    fn render_chart(&self, area: Rect, surface: &mut Surface, ctx: &Context) {
        let theme = &ctx.editor.theme;
        let header = theme.get("ui.text.focus");
        let text = theme.get("ui.text");
        let dim = theme.get("ui.linenr");
        let mark = theme.get("warning");
        let g = &self.game;
        let map = &g.map;
        let ox = area.x + 2;
        let oy = area.y + 4;
        surface.set_string(
            ox,
            area.y,
            &format!(
                "GALAXY CHART — {}   wave {}   salvage {}   score {}",
                g.galaxy.name(),
                g.wave,
                g.credits,
                g.score
            ),
            header,
        );
        surface.set_string(
            ox,
            area.y + 1,
            &format!(
                "{} systems · {} charted · {} cleared · {} — lanes run both ways",
                map.nodes.len(),
                map.charted(),
                map.cleared(),
                g.galaxy.blurb()
            ),
            dim,
        );
        surface.set_string(
            ox,
            area.y + 2,
            "◇ battle  ◆ elite  ☠ boss  ▰ capital  ⌂ depot  ⌗ derelict  ? uncharted",
            dim,
        );
        let reachable = map.reachable();
        // Lanes first, so the systems sit on top of them.
        for (from, lanes) in map.lanes.iter().enumerate() {
            if !map.nodes[from].explored {
                continue;
            }
            let a = map.nodes[from].pos;
            for &to in lanes {
                let b = map.nodes[to].pos;
                if !map.nodes[to].explored || (b.0, b.1) <= (a.0, a.1) {
                    continue;
                }
                let glyph = if b.1 == a.1 {
                    "─"
                } else if b.1 > a.1 {
                    "╲"
                } else {
                    "╱"
                };
                if b.0 > a.0 {
                    let x = ox + (a.0 as u16) * 4 + 3;
                    let y = oy + (a.1 as u16) * 2;
                    surface.set_string(x, y, glyph, dim);
                } else {
                    let x = ox + (a.0 as u16) * 4 + 1;
                    let y = oy + (a.1 as u16) * 2 + 1;
                    surface.set_string(x, y, "│", dim);
                }
            }
        }
        for (i, node) in map.nodes.iter().enumerate() {
            let x = ox + (node.pos.0 as u16) * 4;
            let y = oy + (node.pos.1 as u16) * 2;
            let style = if i == map.at {
                mark
            } else if i == map.cursor {
                header
            } else if reachable.contains(&i) {
                text
            } else {
                dim
            };
            let glyph = if node.explored {
                node.kind.glyph()
            } else {
                "?"
            };
            let frame = if i == map.cursor {
                format!("[{glyph}]")
            } else if i == map.at {
                format!("<{glyph}>")
            } else if node.cleared {
                format!(" {glyph}·")
            } else {
                format!(" {glyph} ")
            };
            surface.set_string(x, y, &frame, style);
        }
        let cursor = map.nodes[map.cursor];
        let footer = oy + (map.height as u16) * 2 + 1;
        surface.set_string(ox, footer, &format!("Target: {}", cursor.label()), header);
        surface.set_string(
            ox,
            footer + 1,
            &format!(
                "Rock: {}   Sector: {}",
                cursor.terrain.blurb(),
                cursor.sector.blurb()
            ),
            dim,
        );
        surface.set_string(
            ox,
            footer + 3,
            "←/→/↑/↓ pick a lane · Enter jump · d land on the world · t roster · o view · n new",
            text,
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
        y += 1;
        // The squad roster: every hull the run has, and which one is flown.
        surface.set_string(ox, y, "SQUAD  (w climbs into the next hull)", header);
        y += 1;
        for (i, wing) in g.squad.iter().enumerate() {
            let flown = i == g.active;
            let style = if !wing.alive {
                dim
            } else if flown {
                header
            } else {
                text
            };
            surface.set_string(
                ox,
                y,
                &format!(
                    "{} {:<6} {:<12} {:<8} L{}  shield {}/{}  E{} R{} P{} C{} M{}  {}",
                    if flown { "▶" } else { " " },
                    wing.name,
                    wing.class.name(),
                    wing.weapon.name(),
                    wing.weapon_level,
                    if flown { g.shield } else { wing.shield },
                    if flown { g.max_shield } else { wing.max_shield },
                    wing.loadout.tier(Part::Engine),
                    wing.loadout.tier(Part::Reactor),
                    wing.loadout.tier(Part::Plating),
                    wing.loadout.tier(Part::Cannon),
                    wing.loadout.tier(Part::Magazine),
                    wing.status()
                ),
                style,
            );
            y += 1;
        }
        y += 1;
        surface.set_string(
            ox,
            y,
            &format!(
                "RACKS  {}   missiles {}",
                g.owned
                    .iter()
                    .enumerate()
                    .map(|(i, w)| format!("{}:{}", i + 1, w.name()))
                    .collect::<Vec<_>>()
                    .join("  "),
                g.missiles
            ),
            text,
        );
        y += 1;
        surface.set_string(
            ox,
            y,
            "THE RACK — press the key to buy, again to sell back",
            header,
        );
        y += 1;
        for (key, arm, price, owned, can) in g.armoury_lines() {
            let style = if g.deck.sidearm == arm {
                header
            } else if can {
                text
            } else {
                dim
            };
            surface.set_string(
                ox,
                y,
                &format!(
                    "{} [{}] {:<18} {:>5}  dmg {:<2} cadence {:<2} reach {:<3} {}",
                    if g.deck.sidearm == arm { "▶" } else { " " },
                    key,
                    arm.name(),
                    price,
                    arm.damage(),
                    arm.cadence(),
                    arm.range(),
                    if owned {
                        "owned — sells for this"
                    } else {
                        ""
                    }
                ),
                style,
            );
            y += 1;
        }
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
        surface.set_string(
            ox,
            y,
            &format!(
                "Parked at {} ({}).  Enter opens the chart · t roster · w next fighter · q quits.",
                g.node.kind.name(),
                g.node.sector.name()
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
        // The view swaps from anywhere: picker, chart, hangar, pause or flight.
        if matches!(key, key!('o')) {
            self.view = match self.view {
                ViewMode::TopDown => ViewMode::Cockpit,
                ViewMode::Cockpit => ViewMode::TopDown,
            };
            if self.running() {
                zmax_event::request_redraw();
            }
            return EventResult::Consumed(None);
        }
        // The roster opens over anything and swallows everything but its own key.
        if matches!(key, key!('t')) {
            self.roster = !self.roster;
            if self.running() {
                self.last = Some(Instant::now());
                zmax_event::request_redraw();
            }
            return EventResult::Consumed(None);
        }
        if self.roster {
            if matches!(key, key!(Enter) | key!(' ')) {
                self.roster = false;
            }
            return EventResult::Consumed(None);
        }
        match self.game.status {
            Status::Select => {
                let hulls = ShipClass::ALL.len();
                match key {
                    key!(Left) | key!('h') => self.pick = (self.pick + hulls - 1) % hulls,
                    key!(Right) | key!('l') => self.pick = (self.pick + 1) % hulls,
                    key!('d') | key!(Tab) => self.diff = (self.diff + 1) % Difficulty::ALL.len(),
                    key!('g') => self.gal = (self.gal + 1) % Galaxy::ALL.len(),
                    key!('m') => {
                        // Fly the war itself rather than the open galaxy.
                        let class = ShipClass::ALL[self.pick];
                        let difficulty = Difficulty::ALL[self.diff];
                        self.game.start_campaign(class, difficulty);
                    }
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
                    key!('4') => {
                        self.pick = 3;
                        self.launch();
                    }
                    key!('5') => {
                        self.pick = 4;
                        self.launch();
                    }
                    key!(Enter) | key!(' ') => self.launch(),
                    _ => {}
                }
            }
            Status::Chart => match key {
                key!(Left) | key!('h') => self.game.steer_chart(-1, 0),
                key!(Right) | key!('l') => self.game.steer_chart(1, 0),
                key!(Up) | key!('k') => self.game.steer_chart(0, -1),
                key!(Down) | key!('j') => self.game.steer_chart(0, 1),
                key!(Tab) => self.game.move_cursor(1),
                key!(Enter) | key!(' ') => {
                    self.game.jump();
                }
                key!('d') => {
                    self.game.land();
                }
                key!('n') => self.restart(),
                _ => {}
            },
            Status::Hangar | Status::Surface => match key {
                // On foot in first person you turn and walk; from above you
                // simply move about the deck.
                key!(Left) | key!('h') => {
                    if self.view == ViewMode::Cockpit {
                        self.game.turn(-1);
                    } else {
                        self.game.walk(-1, 0);
                    }
                }
                key!(Right) | key!('l') => {
                    if self.view == ViewMode::Cockpit {
                        self.game.turn(1);
                    } else {
                        self.game.walk(1, 0);
                    }
                }
                key!(Up) | key!('k') => {
                    if self.view == ViewMode::Cockpit {
                        self.game.step_ahead(1);
                    } else {
                        self.game.walk(0, -1);
                    }
                }
                key!(Down) | key!('j') => {
                    if self.view == ViewMode::Cockpit {
                        self.game.step_ahead(-1);
                    } else {
                        self.game.walk(0, 1);
                    }
                }
                key!(Enter) => {
                    self.game.interact();
                }
                key!(' ') | key!('f') => {
                    // On a world the sidearm comes out; on the deck it is just
                    // another way of using what is in front of you.
                    if self.game.status == Status::Surface {
                        self.game.shoot();
                    } else {
                        self.game.interact();
                    }
                }
                key!('1') => {
                    self.game.draw_sidearm();
                }
                key!('b') => {
                    self.game.parry();
                }
                key!('2') => {
                    self.game.mount();
                }
                key!('e') => {
                    self.game.force_push();
                }
                key!('w') => {
                    self.game.cycle_active();
                }
                key!('f') => {
                    self.game.cycle_formation();
                }
                key!('n') => self.restart(),
                _ => {
                    // Everything else is a terminal key: the rack first, then
                    // the yard's own stock.
                    if let Some(c) = key.char() {
                        if !self.game.trade_sidearm(c) {
                            self.game.buy(c);
                        }
                    }
                }
            },
            _ => match key {
                key!(Left) | key!('h') => self.game.move_ship(-1, 0),
                key!(Right) | key!('l') => self.game.move_ship(1, 0),
                key!(Up) | key!('k') => self.game.move_ship(0, -1),
                key!(Down) | key!('j') => self.game.move_ship(0, 1),
                key!(' ') | key!('f') => self.game.fire(),
                key!('m') => self.game.fire_missiles(),
                key!('[') => self.game.cycle_weapon(-1),
                key!(']') => self.game.cycle_weapon(1),
                key!('1') => {
                    self.game.select_weapon(0);
                }
                key!('2') => {
                    self.game.select_weapon(1);
                }
                key!('3') => {
                    self.game.select_weapon(2);
                }
                key!('4') => {
                    self.game.select_weapon(3);
                }
                key!('5') => {
                    self.game.select_weapon(4);
                }
                key!('6') => {
                    self.game.select_weapon(5);
                }
                key!('7') => {
                    self.game.select_weapon(6);
                }
                key!('8') => {
                    self.game.select_weapon(7);
                }
                key!('9') => {
                    self.game.select_weapon(8);
                }
                key!('0') => {
                    self.game.select_weapon(9);
                }
                key!('x') => self.game.special(),
                key!('z') => {
                    self.game.divert(System::Lasers);
                }
                key!('c') => {
                    self.game.divert(System::Shields);
                }
                key!('v') => {
                    self.game.divert(System::Engines);
                }
                key!('e') => {
                    self.game.use_force(ForcePower::Sense);
                }
                key!('y') => {
                    self.game.use_force(ForcePower::Pull);
                }
                key!('u') => {
                    self.game.use_force(ForcePower::Guided);
                }
                key!('b') => self.game.bomb(),
                key!('p') => self.paused = !self.paused,
                key!('g') => {
                    self.game.cycle_formation();
                }
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
                    if self.game.status == Status::Surface {
                        self.game.ground_tick();
                    } else {
                        self.game.step();
                    }
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
        if self.roster {
            self.render_roster(area, surface, ctx);
            return;
        }
        if self.paused && matches!(self.game.status, Status::Playing | Status::WaveClear) {
            self.render_pause(area, surface, ctx);
            return;
        }
        if self.view == ViewMode::Cockpit
            && matches!(self.game.status, Status::Playing | Status::WaveClear)
        {
            self.render_cockpit(area, surface, ctx);
            return;
        }
        match self.game.status {
            Status::Ceremony => {
                self.render_ceremony(area, surface, ctx);
                return;
            }
            Status::Select => {
                self.render_select(area, surface, ctx);
                return;
            }
            Status::Hangar | Status::Surface => {
                if self.game.shop_open {
                    self.render_hangar(area, surface, ctx);
                } else if self.view == ViewMode::Cockpit {
                    self.render_deck_pov(area, surface, ctx);
                } else {
                    self.render_deck(area, surface, ctx);
                }
                return;
            }
            Status::Chart => {
                self.render_chart(area, surface, ctx);
                return;
            }
            _ => {}
        }
        if area.width < 24 || area.height < 16 {
            surface.set_string(area.x, area.y, "Nova needs a 24×16 window.", text_style);
            return;
        }

        let g = &self.game;
        let ox = area.x + 2;
        let top = area.y + 4;
        // The court is bigger than most windows, so the view is a camera that
        // keeps the hull in the middle of whatever room there is.
        let view_w = (area.width as i16 - 4).clamp(20, W);
        let view_h = (area.height as i16 - 7).clamp(10, H);
        let cam_x = (g.ship.1 - view_w / 2).clamp(0, W - view_w);
        let cam_y = (g.ship.0 - view_h / 2).clamp(0, H - view_h);
        surface.set_string(
            ox,
            area.y,
            &format!(
                "{} · {}  {}  score {}  chain ×{}",
                g.rank().name(),
                match g.mission {
                    Some(mission) => mission.name.to_string(),
                    None => format!("wave {}", g.wave),
                },
                match &g.boss {
                    Some(boss) => format!(
                        "{} · {} · {}",
                        g.sector.name(),
                        g.node.terrain.name(),
                        boss.kind.name()
                    ),
                    None => format!(
                        "{} over {} · {}",
                        g.sector.name(),
                        g.planet.name(),
                        g.objective.label()
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
                "GUN {} L{} [{}/{}]   MSL {}   SHIELD {}{}   BOMB {}   LIVES {}{}",
                g.weapon.name(),
                g.weapon_level,
                g.owned.iter().position(|&w| w == g.weapon).unwrap_or(0) + 1,
                g.owned.len(),
                g.missiles,
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
                "ENERGY {}{} {}{}   PWR L{} S{} E{}   FORCE {}{}{}   WING {}",
                "▰".repeat(pips),
                "▱".repeat(10 - pips),
                g.class.special().name(),
                if g.special_ready() { " READY" } else { "" },
                "▮".repeat(g.power.lasers as usize),
                "▮".repeat(g.power.shields as usize),
                "▮".repeat(g.power.engines as usize),
                "▰".repeat((g.force * 6 / FORCE_MAX) as usize),
                "▱".repeat(6 - (g.force * 6 / FORCE_MAX).min(6) as usize),
                if g.sense > 0 { " SENSE" } else { "" },
                g.squad.iter().filter(|w| w.alive).count()
            ),
            if g.special_ready() {
                header_style
            } else {
                text_style
            },
        );
        // The boss bar takes the fourth line while a boss is up; otherwise the
        // pilot's progress does.
        if let Some(cap) = &g.capital {
            let width = 20i32;
            let filled = (cap.hp.max(0) * width / cap.max_hp.max(1)) as usize;
            surface.set_string(
                ox,
                area.y + 3,
                &format!(
                    "{} {}{}  domes {}  batteries {}  bays {}  {}",
                    cap.kind.name().to_uppercase(),
                    "▰".repeat(filled),
                    "▱".repeat(width as usize - filled),
                    cap.standing(Emplacement::ShieldDome),
                    cap.standing(Emplacement::Turbolaser),
                    cap.standing(Emplacement::HangarBay),
                    if cap.shielded() {
                        "SHIELDED"
                    } else if cap.standing(Emplacement::ExhaustPort) > 0 {
                        "PORT OPEN"
                    } else {
                        "HULL EXPOSED"
                    }
                ),
                boss_style,
            );
        } else {
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
        }

        // Court walls, drawn across whatever the camera can see.
        for c in 0..view_w {
            surface.set_string(ox + c as u16, top, "─", wall_style);
            surface.set_string(ox + c as u16, top + 1 + view_h as u16, "─", wall_style);
        }
        let cell = |r: i16, c: i16| (ox + (c - cam_x) as u16, top + 1 + (r - cam_y) as u16);
        let on_board = |r: i16, c: i16| {
            (cam_y..cam_y + view_h).contains(&r) && (cam_x..cam_x + view_w).contains(&c)
        };

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
            for r in cam_y..cam_y + view_h {
                for c in cam_x..cam_x + view_w {
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
        // The capital ship: plating first, then everything bolted to it.
        if let Some(cap) = &g.capital {
            let shielded = cap.shielded();
            for dr in 0..cap.kind.depth() {
                let span = cap.kind.span(dr);
                for dc in -span..=span {
                    let (r, c) = (cap.pos.0 + dr, cap.pos.1 + dc);
                    if !on_board(r, c) {
                        continue;
                    }
                    let (x, y) = cell(r, c);
                    let edge = dc.abs() == span || dr == cap.kind.depth() - 1;
                    let glyph = if edge { "▛" } else { "▓" };
                    surface.set_string(x, y, glyph, boss_style);
                }
            }
            if shielded {
                // The shield envelope, one row proud of the hull.
                let span = cap.kind.span(cap.kind.depth() - 1) + 1;
                let row = cap.pos.0 + cap.kind.depth();
                for dc in -span..=span {
                    let (r, c) = (row, cap.pos.1 + dc);
                    if on_board(r, c) && (dc + g.tick as i16 / 2) % 3 == 0 {
                        let (x, y) = cell(r, c);
                        surface.set_string(x, y, "˷", beam_style);
                    }
                }
            }
            for part in &cap.parts {
                if part.hp <= 0 {
                    continue;
                }
                let (r, c) = cap.part_cell(part);
                if !on_board(r, c) {
                    continue;
                }
                let (x, y) = cell(r, c);
                let style = if !part.live() {
                    hazard_style
                } else if part.kind == Emplacement::ExhaustPort && !shielded {
                    beam_style
                } else {
                    part_style
                };
                surface.set_string(x, y, part.kind.glyph(), style);
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
        for t in &g.transports {
            if t.away || t.hp <= 0 {
                continue;
            }
            for (i, glyph) in "▭▬▭".chars().enumerate() {
                let (r, c) = (t.pos.0, t.pos.1 + i as i16 - 1);
                if on_board(r, c) {
                    let (x, y) = cell(r, c);
                    surface.set_string(x, y, &glyph.to_string(), power_style);
                }
            }
        }
        for walker in &g.walkers {
            if walker.down {
                continue;
            }
            // Four legs and a body: it reads as a walker even at this size.
            for (i, glyph) in "▟▛▜▙".chars().enumerate() {
                let (r, c) = (walker.pos.0, walker.pos.1 + i as i16 - 1);
                if on_board(r, c) {
                    let (x, y) = cell(r, c);
                    surface.set_string(x, y, &glyph.to_string(), tank_style);
                }
            }
            for i in 0..3 {
                let (r, c) = (walker.pos.0 + 1, walker.pos.1 + i - 1);
                if on_board(r, c) {
                    let (x, y) = cell(r, c);
                    surface.set_string(x, y, "╿", tank_style);
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
            let style = match e.kind {
                EnemyKind::Gunboat | EnemyKind::RepairDroid => tank_style,
                _ if e.charge > 0 => beam_style,
                _ => enemy_style,
            };
            // Three cells of hull, centred on the cell the game tracks.
            for (i, glyph) in e.kind.sprite().chars().enumerate() {
                let col = c + i as i16 - 1;
                if glyph != ' ' && on_board(r, col) {
                    let (x, y) = cell(r, col);
                    surface.set_string(x, y, &glyph.to_string(), style);
                }
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
        for (index, (r, c)) in g.wing_cells() {
            for (i, glyph) in g.squad[index].class.sprite()[1].chars().enumerate() {
                let col = c + i as i16 - 1;
                if glyph != ' ' && on_board(r, col) {
                    let (x, y) = cell(r, col);
                    surface.set_string(x, y, &glyph.to_string(), drone_style);
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
            // The fighter is two rows of hull: canopy and S-foils over engines.
            let sprite = g.class.sprite();
            for (row, line) in sprite.iter().enumerate() {
                let r = g.ship.0 + row as i16 - 1;
                for (i, glyph) in line.chars().enumerate() {
                    let col = g.ship.1 + i as i16 - 1;
                    if glyph != ' ' && on_board(r, col) {
                        let (x, y) = cell(r, col);
                        surface.set_string(x, y, &glyph.to_string(), ship_style);
                    }
                }
            }
        }

        if g.banner > 0 {
            let banner = format!(
                "◈ {} · {} — {} ◈",
                g.sector.name().to_uppercase(),
                g.node.terrain.name(),
                g.node.bonus.label()
            );
            let x = ox + (view_w as u16).saturating_sub(banner.chars().count() as u16) / 2;
            let (_, y) = cell(cam_y + view_h / 2, cam_x);
            surface.set_string(x, y, &banner, header_style);
        }

        // Squadron traffic, newest at the top, under the court.
        for (i, line) in g.chatter.iter().take(2).enumerate() {
            surface.set_string(
                ox,
                top + 2 + view_h as u16 + i as u16 + 1,
                &line.line,
                if i == 0 { header_style } else { wall_style },
            );
        }

        let status_y = top + 2 + view_h as u16;
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
                "fly ←/→/↑/↓ · SPC fire · m torpedoes · 1-0 guns · z/c/v power · e/y/u Force · o cockpit · t roster"
                    .to_string()
            }
            Status::Select
            | Status::Hangar
            | Status::Chart
            | Status::Surface
            | Status::Ceremony => String::new(),
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
        g.start(ShipClass::XWing, Difficulty::Normal, Galaxy::Orion);
        g.node = MapNode {
            pos: (0, 0),
            region: Region::Rim,
            kind: NodeKind::Battle,
            sector: Sector::OpenSpace,
            terrain: TerrainKind::Open,
            bonus: NodeBonus::Refit,
            cleared: false,
            explored: true,
        };
        g.status = Status::Playing;
        g.spawn_wave();
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
        g.weapon = Weapon::QuadLaser;
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
        g.weapon = Weapon::HeavyLaser;
        let col = g.ship.1;
        g.enemies = vec![
            Enemy::new(EnemyKind::TieFighter, (g.ship.0 - 2, col)),
            Enemy::new(EnemyKind::TieFighter, (g.ship.0 - 3, col)),
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
            Enemy::new(EnemyKind::TieFighter, (g.ship.0 - 2, col)),
            Enemy::new(EnemyKind::TieFighter, (g.ship.0 - 3, col)),
        ];
        g.fire();
        g.advance_shots();
        assert_eq!(g.enemies.len(), 1, "the bolt only kills the first hull");
        assert!(g.shots.is_empty(), "and is spent doing it");
    }

    #[test]
    fn homing_missiles_steer_toward_the_nearest_hull() {
        let mut g = flying();
        g.weapon = Weapon::ConcussionMissile;
        g.ship.1 = 30;
        g.enemies = vec![Enemy::new(EnemyKind::TieFighter, (4, 10))];
        g.fire();
        g.advance_shots();
        assert_eq!(g.shots[0].drift, -1, "the missile leans toward the target");
        assert!(g.shots[0].pos.1 < 30, "and has already closed a column");
    }

    #[test]
    fn plasma_damages_the_whole_footprint() {
        let mut g = flying();
        g.weapon = Weapon::ProtonBomb;
        let col = g.ship.1;
        g.enemies = vec![
            Enemy::new(EnemyKind::TieFighter, (g.ship.0 - 2, col - 1)),
            Enemy::new(EnemyKind::TieFighter, (g.ship.0 - 2, col + 1)),
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
        g.start(ShipClass::AWing, Difficulty::Normal, Galaxy::Orion);
        g.launch_next_wave();
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
        g.start(ShipClass::YWing, Difficulty::Normal, Galaxy::Orion);
        g.launch_next_wave();
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
        g.collect(PowerKind::Gun(Weapon::LaserCannon));
        assert_eq!(g.weapon_level, 2, "the carried gun levels up");
        g.collect(PowerKind::Gun(Weapon::HeavyLaser));
        assert_eq!(g.weapon, Weapon::HeavyLaser, "a different gun replaces it");
        for _ in 0..5 {
            g.collect(PowerKind::Gun(Weapon::HeavyLaser));
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
            Enemy::new(EnemyKind::TieFighter, (5, 10)),
            Enemy::new(EnemyKind::Gunboat, (5, 20)),
        ];
        g.enemy_shots = vec![Shot::enemy((10, 10), 0, 1)];
        let bombs = g.bombs;
        g.bomb();
        assert_eq!(g.bombs, bombs - 1, "the bomb is spent");
        assert!(g.enemy_shots.is_empty(), "enemy fire is wiped");
        assert_eq!(g.enemies.len(), 1, "the grunt dies");
        assert_eq!(
            g.enemies[0].hp,
            EnemyKind::Gunboat.hp() - BOMB_DAMAGE,
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
        let mut sniper = g.hatch(EnemyKind::TieDefender, (4, g.ship.1));
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
        let mut splitter = Enemy::new(EnemyKind::VultureDroid, pos);
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
        let mut hurt = g.hatch(EnemyKind::Gunboat, (5, 20));
        hurt.hp = 2;
        g.enemies = vec![g.hatch(EnemyKind::RepairDroid, (5, 22)), hurt];
        g.tick = HEAL_CADENCE;
        g.advance_enemies();
        let tank = g
            .enemies
            .iter()
            .find(|e| e.kind == EnemyKind::Gunboat)
            .expect("the tank is still flying");
        assert_eq!(tank.hp, 3, "the healer welds a hit point back on");
    }

    #[test]
    fn a_diver_that_misses_returns_to_formation_but_a_kamikaze_is_gone() {
        let mut g = flying();
        let mut grunt = Enemy::new(EnemyKind::TieFighter, (4, 8));
        grunt.pos = (H - 1, 8);
        grunt.state = EnemyState::Diving { target_x: 8 };
        let mut kamikaze = Enemy::new(EnemyKind::BuzzDroid, (4, 40));
        kamikaze.pos = (H - 1, 40);
        kamikaze.state = EnemyState::Diving { target_x: 40 };
        g.enemies = vec![grunt, kamikaze];
        g.advance_enemies();
        assert_eq!(g.enemies.len(), 1, "the kamikaze leaves the court");
        assert_eq!(g.enemies[0].kind, EnemyKind::TieFighter);
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
        let mut kamikaze = Enemy::new(EnemyKind::BuzzDroid, (4, g.ship.1));
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
    fn a_boss_system_fields_a_boss_and_they_cycle() {
        let mut g = flying();
        g.wave = 3;
        g.spawn_wave();
        assert!(g.boss.is_none(), "an ordinary system is a formation");
        g.wave = BOSS_EVERY;
        g.node.kind = NodeKind::Boss;
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
        g.node.kind = NodeKind::Boss;
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
        g.owned = vec![Weapon::LaserCannon, Weapon::MassDriver];
        let swap = g
            .shop_lines()
            .into_iter()
            .find(|l| l.entry == ShopEntry::Consumable(Stock::GunSwap))
            .expect("the swap is on the list");
        assert!(g.buy(swap.key));
        assert_eq!(
            g.weapon,
            Weapon::MassDriver,
            "the racks rotate to the next gun"
        );
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
        g.start(ShipClass::AWing, Difficulty::Insane, Galaxy::Orion);
        g.launch_next_wave();
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
        g.start(ShipClass::XWing, Difficulty::Normal, Galaxy::Orion);
        g.node = MapNode {
            pos: (0, 0),
            region: Region::Rim,
            kind: NodeKind::Battle,
            sector: Sector::OpenSpace,
            terrain: kind,
            bonus: NodeBonus::Refit,
            cleared: false,
            explored: true,
        };
        g.status = Status::Playing;
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
    fn the_chart_flies_the_lane_you_pick() {
        let mut g = flying();
        g.check_end();
        for _ in 0..INTERMISSION_TICKS {
            g.step();
        }
        assert_eq!(g.status, Status::Hangar, "a cleared system docks the squad");
        g.open_chart();
        assert_eq!(g.status, Status::Chart);
        let lanes = g.map.reachable();
        assert!(!lanes.is_empty(), "there is always somewhere to fly");
        g.move_cursor(1);
        let picked = g.map.nodes[g.map.cursor];
        assert!(
            lanes.contains(&g.map.cursor),
            "the cursor stays on the lanes"
        );
        assert!(g.jump(), "and the lane is flown");
        assert_eq!(g.node, g.map.here(), "the squad is parked where it jumped");
        if picked.kind.fights() {
            assert_eq!(g.status, Status::Playing, "a fight starts straight away");
            assert_eq!(g.sector, picked.sector, "in the system's own sector");
        } else {
            assert_eq!(g.status, Status::Hangar, "a depot or hulk parks the squad");
        }
    }

    #[test]
    fn a_salvage_cache_pays_out_on_arrival() {
        let mut g = flying();
        g.credits = 0;
        g.node = MapNode {
            pos: (0, 0),
            region: Region::Rim,
            kind: NodeKind::Battle,
            sector: Sector::OpenSpace,
            terrain: TerrainKind::Open,
            bonus: NodeBonus::Cache(750),
            cleared: false,
            explored: true,
        };
        g.spawn_wave();
        assert_eq!(g.credits, 750, "the cache is banked when the wave starts");
    }

    #[test]
    fn an_armoury_stop_hands_over_its_gun() {
        let mut g = flying();
        g.weapon = Weapon::LaserCannon;
        g.weapon_level = 1;
        g.node = MapNode {
            pos: (0, 0),
            region: Region::Rim,
            kind: NodeKind::Battle,
            sector: Sector::OpenSpace,
            terrain: TerrainKind::Open,
            bonus: NodeBonus::Armoury(Weapon::ProtonBomb),
            cleared: false,
            explored: true,
        };
        g.spawn_wave();
        assert_eq!(g.weapon, Weapon::ProtonBomb, "the crate is fitted");
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
        g.node = MapNode {
            pos: (0, 0),
            region: Region::Rim,
            kind: NodeKind::Battle,
            sector: Sector::OpenSpace,
            terrain: TerrainKind::Tunnel,
            bonus: NodeBonus::Refit,
            cleared: false,
            explored: true,
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
        g.start(ShipClass::YWing, Difficulty::Hard, Galaxy::Orion);
        g.launch_next_wave();
        for i in 0..3_000 {
            if matches!(g.status, Status::Hangar | Status::Chart) {
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
        g.weapon = Weapon::LaserCannon;
        let blaster = g.cadence();
        g.weapon = Weapon::RepeatingBlaster;
        assert!(g.cadence() < blaster, "the machine gun barely pauses");
        g.weapon = Weapon::MassDriver;
        assert!(g.cadence() > blaster, "the rail gun takes its time");
        g.weapon = Weapon::RepeatingBlaster;
        g.weapon_level = 3;
        g.fire();
        assert_eq!(g.shots.len(), 3, "and walks three rounds across at level 3");
    }

    #[test]
    fn a_rocket_takes_the_neighbours_with_it() {
        let mut g = flying();
        g.weapon = Weapon::RocketPod;
        let (row, col) = (g.ship.0 - 2, g.ship.1);
        g.enemies = vec![
            Enemy::new(EnemyKind::TieFighter, (row, col)),
            Enemy::new(EnemyKind::TieFighter, (row, col + 1)),
            Enemy::new(EnemyKind::TieFighter, (row - 1, col - 1)),
        ];
        g.fire();
        g.advance_shots();
        assert!(g.enemies.is_empty(), "the blast clears the cells around it");
    }

    #[test]
    fn a_flak_shell_bursts_into_a_fan() {
        let mut g = flying();
        g.weapon = Weapon::Flechette;
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
        g.weapon = Weapon::MassDriver;
        let col = g.ship.1;
        g.enemies = (2..=5)
            .map(|dr| Enemy::new(EnemyKind::Gunboat, (g.ship.0 - dr, col)))
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
        g.weapon = Weapon::ArcCaster;
        g.weapon_level = 2;
        let (row, col) = (g.ship.0 - 2, g.ship.1);
        g.enemies = vec![
            Enemy::new(EnemyKind::TieFighter, (row, col)),
            Enemy::new(EnemyKind::TieFighter, (row, col + 3)),
            Enemy::new(EnemyKind::TieFighter, (row - 1, col + 5)),
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
            g.node = MapNode {
                pos: (0, 0),
                region: Region::Rim,
                kind: NodeKind::Battle,
                sector,
                terrain: TerrainKind::Open,
                bonus: NodeBonus::Refit,
                cleared: false,
                explored: true,
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
                g.start(ShipClass::XWing, Difficulty::Normal, Galaxy::Orion);
                g.status = Status::Playing;
                g.node = MapNode {
                    pos: (0, 0),
                    region: Region::Rim,
                    kind: NodeKind::Battle,
                    sector,
                    terrain,
                    bonus: NodeBonus::Refit,
                    cleared: false,
                    explored: true,
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

#[cfg(test)]
mod squad_tests {
    use super::tests::flying;
    use super::*;

    fn docked() -> Game {
        let mut g = flying();
        g.status = Status::Hangar;
        g.credits = 20_000;
        g
    }

    fn line(g: &Game, stock: Stock) -> ShopLine {
        g.shop_lines()
            .into_iter()
            .find(|l| l.entry == ShopEntry::Consumable(stock))
            .expect("the hangar lists it")
    }

    #[test]
    fn a_run_starts_with_one_hull_and_the_hangar_sells_more() {
        let mut g = docked();
        assert_eq!(g.squad.len(), 1, "one hull off the line");
        let hull = line(&g, Stock::Hull);
        assert!(g.buy(hull.key), "a second hull is for sale");
        assert_eq!(g.squad.len(), 2, "and joins the squad");
        assert_ne!(
            g.squad[1].class, g.squad[0].class,
            "a squad is built out of different hulls"
        );
        while g.squad.len() < MAX_SQUAD {
            let hull = line(&g, Stock::Hull);
            assert!(g.buy(hull.key));
        }
        assert!(
            !line(&g, Stock::Hull).available,
            "and it stops at a full squad"
        );
    }

    #[test]
    fn the_pilot_can_climb_into_another_hull_in_the_hangar() {
        let mut g = docked();
        let hull = line(&g, Stock::Hull);
        g.buy(hull.key);
        g.weapon = Weapon::MassDriver;
        let flown = g.class;
        assert!(g.cycle_active(), "the pilot climbs across");
        assert_eq!(g.active, 1);
        assert_ne!(g.class, flown, "into a different hull");
        assert_eq!(g.weapon, Weapon::LaserCannon, "which carries its own gun");
        assert!(g.cycle_active(), "and back again");
        assert_eq!(g.active, 0);
        assert_eq!(
            g.weapon,
            Weapon::MassDriver,
            "the first hull kept its rail gun"
        );
    }

    #[test]
    fn wingmen_fly_alongside_and_fire_on_their_own() {
        let mut g = docked();
        g.buy(line(&g, Stock::Hull).key);
        g.status = Status::Playing;
        let wings = g.wing_cells();
        assert_eq!(wings.len(), 1, "the second hull rides as a wingman");
        assert_ne!(wings[0].1 .1, g.ship.1, "off to one side of the hull");
        g.tick = WING_CADENCE;
        g.advance_wings();
        assert_eq!(g.shots.len(), 1, "and puts its own fire up the court");
    }

    #[test]
    fn a_wingman_can_be_shot_down_and_rescued() {
        let mut g = docked();
        g.buy(line(&g, Stock::Hull).key);
        g.status = Status::Playing;
        let (index, cell) = g.wing_cells()[0];
        let pips = g.squad[index].shield;
        g.enemy_shots = vec![Shot::enemy((cell.0 - 1, cell.1), 0, 1)];
        for _ in 0..pips {
            g.advance_enemy_shots();
            g.enemy_shots = vec![Shot::enemy((cell.0 - 1, cell.1), 0, 1)];
        }
        assert!(!g.squad[index].alive, "the wingman goes down");
        assert!(g.wing_cells().is_empty(), "and stops flying");
        g.status = Status::Hangar;
        assert!(g.buy(line(&g, Stock::Rescue).key), "a yard puts it back up");
        assert!(g.squad[index].alive);
        assert_eq!(g.squad[index].shield, g.squad[index].max_shield);
    }

    #[test]
    fn guns_go_into_the_racks_and_can_be_swapped_mid_fight() {
        let mut g = flying();
        assert_eq!(g.owned, vec![Weapon::LaserCannon], "one gun to start with");
        g.collect(PowerKind::Gun(Weapon::MassDriver));
        g.collect(PowerKind::Gun(Weapon::RepeatingBlaster));
        assert_eq!(g.owned.len(), 3, "picked-up guns are kept");
        assert_eq!(
            g.weapon,
            Weapon::RepeatingBlaster,
            "and the last one is fitted"
        );
        assert!(g.select_weapon(0), "any rack slot can be selected");
        assert_eq!(g.weapon, Weapon::LaserCannon);
        g.cycle_weapon(-1);
        assert_eq!(
            g.weapon,
            Weapon::RepeatingBlaster,
            "and the racks wrap around"
        );
        assert!(!g.select_weapon(9), "empty slots are refused");
    }

    #[test]
    fn a_gun_picked_up_twice_levels_up_rather_than_stacking() {
        let mut g = flying();
        g.collect(PowerKind::Gun(Weapon::Flechette));
        let level = g.weapon_level;
        g.collect(PowerKind::Gun(Weapon::Flechette));
        assert_eq!(g.owned.len(), 2, "the racks hold one of each");
        assert_eq!(g.weapon_level, level + 1, "the second one is an upgrade");
    }

    #[test]
    fn the_launcher_fires_seeking_rounds_off_its_own_ammunition() {
        let mut g = flying();
        g.enemies = vec![Enemy::new(EnemyKind::TieFighter, (4, g.ship.1 - 10))];
        let ammo = g.missiles;
        g.fire_missiles();
        assert_eq!(g.missiles, ammo - 1, "a round leaves the launcher");
        assert_eq!(g.shots.len(), MISSILE_SALVO as usize, "as a salvo");
        assert!(g.shots.iter().all(|s| s.homing), "the rounds seek");
        assert!(g.shots.iter().all(|s| s.splash > 0), "and blast on impact");
        g.advance_shots();
        assert!(
            g.shots.iter().all(|s| s.drift <= 0),
            "leaning toward the hull off to port"
        );
        g.missiles = 0;
        g.shots.clear();
        g.fire_missiles();
        assert!(g.shots.is_empty(), "an empty launcher fires nothing");
    }

    #[test]
    fn the_hangar_sells_missile_packs() {
        let mut g = docked();
        g.missiles = 0;
        assert!(g.buy(line(&g, Stock::Missiles).key));
        assert_eq!(g.missiles, MISSILE_PACK, "a pack is loaded");
    }
}

#[cfg(test)]
mod galaxy_tests {
    use super::tests::flying;
    use super::*;

    #[test]
    fn every_galaxy_lays_out_a_chart_that_can_be_flown() {
        for galaxy in Galaxy::ALL {
            let map = StarMap::generate(galaxy, 7);
            assert!(
                map.nodes.len() >= galaxy.systems() * 8 / 10,
                "{} fills its chart",
                galaxy.name()
            );
            assert!(
                !map.reachable().is_empty(),
                "there is a lane out of the rim"
            );
            for node in &map.nodes {
                assert!(
                    galaxy.sectors().contains(&node.sector),
                    "{} only fields its own sectors",
                    galaxy.name()
                );
                assert!(
                    galaxy.terrains().contains(&node.terrain) || node.kind == NodeKind::Capital,
                    "{} only fields its own rock outside a capital system",
                    galaxy.name()
                );
            }
        }
    }

    #[test]
    fn lanes_run_both_ways_so_a_depot_can_be_flown_back_to() {
        let mut map = StarMap::generate(Galaxy::Orion, 3);
        let start = map.at;
        let out = map.reachable()[0];
        map.cursor = out;
        assert!(map.jump().is_some(), "the lane is flown");
        assert_eq!(map.at, out);
        assert!(
            map.reachable().contains(&start),
            "and the way back is still open"
        );
    }

    #[test]
    fn the_chart_cursor_only_walks_the_lanes_out_of_here() {
        let mut g = flying();
        g.status = Status::Chart;
        let lanes = g.map.reachable();
        for _ in 0..lanes.len() * 2 {
            g.move_cursor(1);
            assert!(
                lanes.contains(&g.map.cursor),
                "the cursor never leaves the lanes"
            );
        }
    }

    #[test]
    fn a_depot_repairs_the_squad_without_a_fight() {
        let mut g = flying();
        g.status = Status::Chart;
        g.shield = 0;
        g.squad.push(Wing::new("Two", ShipClass::AWing));
        g.squad[1].alive = false;
        let depot = MapNode {
            pos: (1, 0),
            region: Region::Rim,
            kind: NodeKind::Depot,
            sector: Sector::OpenSpace,
            terrain: TerrainKind::Open,
            bonus: NodeBonus::Refit,
            cleared: false,
            explored: true,
        };
        let at = g.map.reachable()[0];
        g.map.nodes[at] = depot;
        g.map.cursor = at;
        assert!(g.jump(), "the lane is flown");
        assert_eq!(g.status, Status::Hangar, "a yard is not a fight");
        assert_eq!(g.shield, g.max_shield, "the hull is patched up");
        assert!(g.squad[1].alive, "and the wing is back in the air");
    }

    #[test]
    fn a_derelict_pays_salvage_and_leaves_a_gun_behind() {
        let mut g = flying();
        g.status = Status::Chart;
        g.credits = 0;
        let hulk = MapNode {
            pos: (1, 0),
            region: Region::Rim,
            kind: NodeKind::Derelict,
            sector: Sector::OpenSpace,
            terrain: TerrainKind::Open,
            bonus: NodeBonus::Armoury(Weapon::MassDriver),
            cleared: false,
            explored: true,
        };
        let at = g.map.reachable()[0];
        g.map.nodes[at] = hulk;
        g.map.cursor = at;
        assert!(g.jump());
        assert!(g.credits > 0, "the hulk is worth stripping");
        assert_eq!(g.weapon, Weapon::MassDriver, "and the gun aboard is fitted");
        assert!(g.owned.contains(&Weapon::MassDriver), "into the racks");
    }

    #[test]
    fn an_elite_system_is_tougher_and_pays_double() {
        let mut g = flying();
        let plain = g.wave_armour();
        g.credits = 0;
        g.award(100);
        let plain_salvage = g.credits;
        g.node.kind = NodeKind::Elite;
        assert_eq!(g.wave_armour(), plain + 2, "elite hulls carry more armour");
        g.credits = 0;
        g.combo = 1;
        g.award(100);
        assert_eq!(g.credits, plain_salvage * 2, "and pay twice as much");
    }

    #[test]
    fn galaxies_bias_the_run_they_are_flown_in() {
        let mut hive = Game::new(2);
        hive.start(ShipClass::XWing, Difficulty::Normal, Galaxy::Hive);
        let mut abyss = Game::new(2);
        abyss.start(ShipClass::XWing, Difficulty::Normal, Galaxy::Abyss);
        assert!(
            hive.wave_armour() < abyss.wave_armour(),
            "the abyss armours everything the hive does not"
        );
        assert_eq!(Galaxy::Hive.swarm(), 1, "and the hive fields deeper waves");
        assert!(
            Galaxy::Forge.salvage_bonus() > Galaxy::Orion.salvage_bonus(),
            "the forge is where the salvage is"
        );
    }

    #[test]
    fn a_run_across_a_galaxy_never_panics() {
        for galaxy in Galaxy::ALL {
            let mut g = Game::new(13);
            g.start(ShipClass::XWing, Difficulty::Normal, galaxy);
            for i in 0..4_000 {
                match g.status {
                    Status::Lost => break,
                    Status::Hangar => {
                        while let Some(l) = g.shop_lines().into_iter().find(|l| l.available) {
                            g.buy(l.key);
                        }
                        g.open_chart();
                    }
                    Status::Chart => {
                        g.move_cursor(1);
                        g.jump();
                    }
                    _ => {
                        if i % 3 == 0 {
                            g.fire();
                        }
                        if i % 31 == 0 {
                            g.fire_missiles();
                        }
                        g.move_ship(if i % 5 < 2 { 1 } else { -1 }, 0);
                        g.step();
                    }
                }
                assert!(g.active < g.squad.len(), "the squad index stays sane");
            }
        }
    }
}

#[cfg(test)]
mod capital_tests {
    use super::tests::flying;
    use super::*;

    /// A court with a capital ship in it, and nothing else.
    fn against(kind: CapitalKind) -> Game {
        let mut g = flying();
        g.node.kind = NodeKind::Capital;
        g.capital = Some(Capital::new(kind, 0, 0));
        g.enemies.clear();
        g
    }

    fn cell_of(g: &Game, kind: Emplacement) -> (i16, i16) {
        let cap = g.capital.as_ref().expect("a capital is in the system");
        let part = cap
            .parts
            .iter()
            .find(|p| p.kind == kind && p.hp > 0)
            .unwrap_or_else(|| panic!("the ship carries a {}", kind.name()));
        cap.part_cell(part)
    }

    #[test]
    fn a_capital_system_fields_a_capital_and_its_fighter_screen() {
        let mut g = flying();
        g.node.kind = NodeKind::Capital;
        g.node.terrain = TerrainKind::Trench;
        g.spawn_wave();
        let cap = g
            .capital
            .as_ref()
            .expect("the station is holding the system");
        assert_eq!(cap.kind, CapitalKind::DeathStar, "a trench means a station");
        assert!(
            g.enemies.iter().all(|e| e.kind == EnemyKind::TieAdvanced),
            "with a screen of fighters out in front of it"
        );
        g.node.terrain = TerrainKind::Open;
        g.node.region = Region::Core;
        g.spawn_wave();
        assert_eq!(
            g.capital.as_ref().unwrap().kind,
            CapitalKind::StarDestroyer,
            "the core fields wedges"
        );
    }

    #[test]
    fn the_hull_cannot_be_touched_while_a_dome_stands() {
        let mut g = against(CapitalKind::StarDestroyer);
        let hull = {
            let cap = g.capital.as_ref().unwrap();
            (cap.pos.0 + cap.kind.depth() - 1, cap.pos.1 + 12)
        };
        let before = g.capital.as_ref().unwrap().hp;
        assert!(
            g.hit_capital(&Shot::bolt(hull, 0, 20)),
            "the plating is hit"
        );
        assert_eq!(
            g.capital.as_ref().unwrap().hp,
            before,
            "but the shields hold it out"
        );
        for part in g.capital.as_mut().unwrap().parts.iter_mut() {
            if part.kind == Emplacement::ShieldDome {
                part.hp = 0;
            }
        }
        assert!(
            !g.capital.as_ref().unwrap().shielded(),
            "the domes are down"
        );
        g.hit_capital(&Shot::bolt(hull, 0, 20));
        assert!(
            g.capital.as_ref().unwrap().hp < before,
            "and now the hull takes it"
        );
    }

    #[test]
    fn an_ion_bolt_scrambles_a_dome_instead_of_breaking_it() {
        let mut g = against(CapitalKind::StarDestroyer);
        let dome = cell_of(&g, Emplacement::ShieldDome);
        let hp = g.capital.as_ref().unwrap().parts[1].hp;
        g.hit_capital(&Shot::ion(dome, 3));
        let cap = g.capital.as_ref().unwrap();
        let part = cap
            .parts
            .iter()
            .find(|p| p.kind == Emplacement::ShieldDome)
            .unwrap();
        assert_eq!(part.hp, hp, "the dome is not broken");
        assert!(part.ion > 0, "it is scrambled");
        assert!(!part.live(), "and offline while it is");
    }

    #[test]
    fn a_run_down_the_trench_ends_the_station() {
        let mut g = against(CapitalKind::DeathStar);
        let port = cell_of(&g, Emplacement::ExhaustPort);
        let full = g.capital.as_ref().unwrap().hp;
        g.hit_capital(&Shot::torpedo(port, 30));
        assert!(
            g.capital.as_ref().unwrap().hp > 0,
            "shielded, the blast is contained"
        );
        assert!(
            g.capital.as_ref().unwrap().hp < full,
            "though it still shakes the station"
        );
        for part in g.capital.as_mut().unwrap().parts.iter_mut() {
            if part.kind == Emplacement::ShieldDome {
                part.hp = 0;
            }
        }
        let port = cell_of(&g, Emplacement::ExhaustPort);
        g.hit_capital(&Shot::torpedo(port, 30));
        assert!(
            g.capital.as_ref().unwrap().hp <= 0,
            "with the shields down the port takes the whole station"
        );
        g.check_end();
        assert!(g.capital.is_none(), "and it is gone");
        assert_eq!(g.status, Status::WaveClear, "the system is clear");
        assert!(g.score >= 2_000, "the bounty is paid");
    }

    #[test]
    fn the_batteries_fire_heavy_and_the_bays_launch_fighters() {
        let mut g = against(CapitalKind::StarDestroyer);
        for part in g.capital.as_mut().unwrap().parts.iter_mut() {
            part.cooldown = 0;
        }
        g.advance_capital();
        assert!(!g.enemy_shots.is_empty(), "the batteries open up");
        assert!(
            g.enemy_shots.iter().any(|s| s.damage >= 2),
            "and they hit for two pips"
        );
        assert!(
            g.enemies.iter().any(|e| e.kind == EnemyKind::TieAdvanced),
            "the bays put fighters in the air"
        );
    }

    #[test]
    fn a_tractor_beam_drags_the_hull_in() {
        let mut g = against(CapitalKind::StarDestroyer);
        {
            let cap = g.capital.as_mut().unwrap();
            cap.pos.1 = W - 12;
            cap.tick = 2;
            for part in cap.parts.iter_mut() {
                // Silence everything but the beam so only the pull moves us.
                if part.kind != Emplacement::TractorBeam {
                    part.hp = 0;
                }
            }
        }
        g.ship.1 = 10;
        g.advance_capital();
        assert_eq!(g.ship.1, 11, "the beam walks the hull toward the ship");
    }

    #[test]
    fn losing_the_engines_pins_the_ship_and_losing_the_tower_slows_its_guns() {
        let mut g = against(CapitalKind::StarDestroyer);
        {
            let cap = g.capital.as_mut().unwrap();
            cap.tick = 5;
            cap.pos.1 = W / 2;
        }
        g.advance_capital();
        let moved = g.capital.as_ref().unwrap().pos.1;
        assert_ne!(moved, W / 2, "under way it holds station across the court");
        let quick = g.capital.as_ref().unwrap().cadence();
        for part in g.capital.as_mut().unwrap().parts.iter_mut() {
            if matches!(
                part.kind,
                Emplacement::EngineBank | Emplacement::CommandTower
            ) {
                part.hp = 0;
            }
        }
        assert!(
            g.capital.as_ref().unwrap().cadence() > quick,
            "a headless ship is half as quick on the trigger"
        );
        let pinned = g.capital.as_ref().unwrap().pos.1;
        for _ in 0..12 {
            g.advance_capital();
        }
        assert_eq!(
            g.capital.as_ref().unwrap().pos.1,
            pinned,
            "and with its engines gone it cannot move at all"
        );
    }

    #[test]
    fn torpedoes_and_ion_bolts_are_in_the_racks() {
        let mut g = flying();
        for weapon in [Weapon::ProtonTorpedo, Weapon::IonCannon] {
            g.weapon = weapon;
            g.weapon_level = 1;
            g.fire_cooldown = 0;
            g.shots.clear();
            g.fire();
            assert!(!g.shots.is_empty(), "{} fires", weapon.name());
        }
        g.weapon = Weapon::ProtonTorpedo;
        let slow = g.cadence();
        g.weapon = Weapon::RepeatingBlaster;
        assert!(slow > g.cadence(), "a torpedo tube is slow to reload");
        g.weapon = Weapon::ProtonTorpedo;
        g.fire_cooldown = 0;
        g.shots.clear();
        g.fire();
        assert!(g.shots.iter().all(|s| s.homing), "torpedoes track");
        assert!(g.shots.iter().all(|s| s.splash > 0), "and blast on impact");
    }

    #[test]
    fn a_trench_is_one_lane_between_two_walls() {
        let trench = Terrain::new(TerrainKind::Trench, 5);
        let (left, right) = trench.channel(4);
        assert!(left > 1, "armour to port");
        assert!(right < W - 2, "armour to starboard");
        assert!(right - left <= 24, "and only a lane between them");
    }

    #[test]
    fn the_chart_is_a_grid_that_can_be_roamed_and_charts_itself() {
        let mut g = flying();
        g.status = Status::Chart;
        let charted = g.map.charted();
        assert!(charted < g.map.nodes.len(), "most of the galaxy is dark");
        assert!(g.map.nodes.len() >= 60, "and it is a big galaxy");
        let start = g.map.at;
        g.steer_chart(1, 0);
        assert!(
            g.map.nodes[g.map.cursor].pos.0 >= g.map.nodes[start].pos.0,
            "steering right looks deeper in"
        );
        assert!(g.jump(), "the lane is flown");
        assert!(
            g.map.charted() > charted,
            "and arriving lights up the neighbours"
        );
    }

    #[test]
    fn regions_get_meaner_the_deeper_in_they_are() {
        assert!(
            Region::Deep > Region::Rim,
            "the deep is deeper than the rim"
        );
        assert!(
            Region::Deep.armour() > Region::Rim.armour(),
            "and everything out there is armoured"
        );
        assert_eq!(Region::of_column(0, 10), Region::Rim);
        assert_eq!(Region::of_column(9, 10), Region::Deep);
    }

    #[test]
    fn a_capital_fight_plays_out_without_panicking() {
        for kind in [
            CapitalKind::ImperialFrigate,
            CapitalKind::StarDestroyer,
            CapitalKind::DeathStar,
        ] {
            let mut g = against(kind);
            g.weapon = Weapon::ProtonTorpedo;
            for i in 0..2_000 {
                if g.status != Status::Playing {
                    break;
                }
                if i % 4 == 0 {
                    g.fire();
                }
                if i % 37 == 0 {
                    g.fire_missiles();
                }
                g.move_ship(if i % 9 < 4 { 1 } else { -1 }, 0);
                g.step();
                assert!(
                    (1..W - 1).contains(&g.ship.1),
                    "the hull stays on the court"
                );
            }
        }
    }
}

#[cfg(test)]
mod fleet_tests {
    use super::tests::flying;
    use super::*;

    #[test]
    fn five_fighters_are_on_the_flight_line() {
        assert_eq!(ShipClass::ALL.len(), 5, "A-wing through freighter");
        let names: Vec<&str> = ShipClass::ALL.iter().map(|c| c.name()).collect();
        assert!(names.contains(&"X-wing"));
        assert!(names.contains(&"Y-wing"));
        assert!(names.contains(&"B-wing"));
        assert!(
            ShipClass::AWing.speed() > ShipClass::YWing.speed(),
            "the A-wing is the quick one"
        );
        assert!(
            ShipClass::BWing.damage() > ShipClass::XWing.damage(),
            "the B-wing hits hardest"
        );
        assert!(
            ShipClass::Freighter.max_shield() > ShipClass::XWing.max_shield(),
            "and the freighter carries the most plating"
        );
    }

    #[test]
    fn every_fighter_can_be_launched() {
        for class in ShipClass::ALL {
            let mut g = Game::new(4);
            g.start(class, Difficulty::Normal, Galaxy::Orion);
            assert_eq!(g.class, class, "{} launches", class.name());
            assert_eq!(g.squad[0].class, class, "as the flight leader");
            g.launch_next_wave();
            g.fire();
            assert!(!g.shots.is_empty(), "{} has guns", class.name());
        }
    }

    #[test]
    fn power_is_split_three_ways_and_can_be_diverted() {
        let mut g = flying();
        assert_eq!(
            g.power.lasers + g.power.shields + g.power.engines,
            POWER_PIPS
        );
        let damage = g.gun_damage();
        assert!(g.divert(System::Lasers), "a pip goes to the cannons");
        assert_eq!(g.gun_damage(), damage + 1, "and they hit harder for it");
        assert_eq!(
            g.power.lasers + g.power.shields + g.power.engines,
            POWER_PIPS,
            "the reactor puts out no more than it has"
        );
        let thrust = g.thrust();
        for _ in 0..POWER_PIPS {
            g.divert(System::Engines);
        }
        assert!(g.thrust() > thrust, "everything to the engines is faster");
        assert!(!g.divert(System::Engines), "and there is no more to give");
    }

    #[test]
    fn deflectors_knit_themselves_back_while_they_have_the_power() {
        let mut g = flying();
        g.shield = 0;
        g.power = Power {
            lasers: 0,
            shields: POWER_PIPS,
            engines: 0,
        };
        let cadence = (SHIELD_KNIT_TICKS / POWER_PIPS).max(30);
        g.tick = cadence - 1;
        g.tick_timers();
        assert_eq!(g.shield, 1, "a pip knits itself back");
        g.shield = 0;
        g.power = Power {
            lasers: POWER_PIPS,
            shields: 0,
            engines: 0,
        };
        for _ in 0..cadence * 2 {
            g.tick_timers();
        }
        assert_eq!(g.shield, 0, "with no power to them, nothing comes back");
    }

    #[test]
    fn the_imperial_line_runs_from_frigate_to_command_ship() {
        let frigate = Capital::new(CapitalKind::ImperialFrigate, 0, 0);
        let destroyer = Capital::new(CapitalKind::StarDestroyer, 0, 0);
        let command = Capital::new(CapitalKind::SuperDestroyer, 0, 0);
        assert!(destroyer.max_hp > frigate.max_hp, "a destroyer is heavier");
        assert!(
            command.max_hp > destroyer.max_hp,
            "the command ship heavier still"
        );
        assert!(
            command.standing(Emplacement::Turbolaser) > destroyer.standing(Emplacement::Turbolaser),
            "and carries more batteries"
        );
        assert!(
            command.kind.span(command.kind.depth() - 1)
                > destroyer.kind.span(destroyer.kind.depth() - 1),
            "across a wider hull"
        );
        assert_eq!(
            CapitalKind::DeathStar.name(),
            "Death Star",
            "and the station is what it is"
        );
    }

    #[test]
    fn the_core_worlds_field_the_command_ship() {
        let mut g = flying();
        g.node.kind = NodeKind::Capital;
        g.node.terrain = TerrainKind::Open;
        g.node.region = Region::Deep;
        g.spawn_wave();
        assert_eq!(
            g.capital.as_ref().unwrap().kind,
            CapitalKind::SuperDestroyer,
            "the deepest systems are held by the command ship"
        );
        g.node.region = Region::Rim;
        g.spawn_wave();
        assert_eq!(
            g.capital.as_ref().unwrap().kind,
            CapitalKind::ImperialFrigate,
            "the rim only ever fields a picket"
        );
    }

    #[test]
    fn the_squadron_flies_under_its_own_callsigns() {
        let mut g = flying();
        g.status = Status::Hangar;
        g.credits = 20_000;
        assert_eq!(g.squad[0].name, "Red Leader");
        let hull = g
            .shop_lines()
            .into_iter()
            .find(|l| l.entry == ShopEntry::Consumable(Stock::Hull))
            .expect("the yard has another fighter");
        assert!(g.buy(hull.key));
        assert_eq!(g.squad[1].name, "Red Two", "the wing fills out in order");
    }
}

#[cfg(test)]
mod force_tests {
    use super::tests::flying;
    use super::*;

    #[test]
    fn the_force_builds_with_the_flying_and_is_spent_on_powers() {
        let mut g = flying();
        g.force = 0;
        g.award(50);
        assert_eq!(g.force, FORCE_PER_KILL, "a kill puts some of it back");
        g.force = FORCE_MAX;
        assert!(
            g.use_force(ForcePower::Sense),
            "there is enough to reach for"
        );
        assert_eq!(g.force, FORCE_MAX - SENSE_COST, "and it costs");
        assert_eq!(g.sense, SENSE_TICKS, "senses are stretched out");
        g.force = 0;
        assert!(
            !g.use_force(ForcePower::Sense),
            "an empty pilot reaches for nothing"
        );
    }

    #[test]
    fn stretched_out_senses_halve_the_speed_of_imperial_fire() {
        let mut g = flying();
        g.sense = SENSE_TICKS;
        g.tick = 1;
        g.enemy_shots = vec![Shot::enemy((4, 20), 0, 1)];
        g.advance_enemy_shots();
        assert_eq!(g.enemy_shots[0].pos.0, 4, "on the odd tick nothing moves");
        g.tick = 2;
        g.advance_enemy_shots();
        assert_eq!(g.enemy_shots[0].pos.0, 5, "on the even tick it crawls on");
    }

    #[test]
    fn a_force_pull_brings_the_pickups_in() {
        let mut g = flying();
        g.force = FORCE_MAX;
        g.powerups = vec![Powerup {
            pos: (2, 4),
            kind: PowerKind::Bomb,
        }];
        assert!(g.use_force(ForcePower::Pull));
        assert_eq!(g.powerups[0].pos, g.ship, "it comes to the hull");
    }

    #[test]
    fn letting_go_of_the_targeting_computer_puts_a_salvo_down_the_shaft() {
        let mut g = flying();
        g.node.kind = NodeKind::Capital;
        g.capital = Some(Capital::new(CapitalKind::DeathStar, 0, 0));
        for part in g.capital.as_mut().unwrap().parts.iter_mut() {
            if part.kind == Emplacement::ShieldDome {
                part.hp = 0;
            }
        }
        let port = g.weak_point().expect("the port is open");
        g.force = FORCE_MAX;
        assert!(g.use_force(ForcePower::Guided), "he lets go");
        g.ship.1 = 8;
        let plain = g.gun_damage() + 4;
        g.fire_missiles();
        assert!(!g.guided, "the salvo spends it");
        assert!(
            g.shots.iter().all(|s| s.damage > plain),
            "a guided torpedo hits far harder"
        );
        assert!(
            g.shots.iter().all(|s| s.drift == (port.1 - 8).signum()),
            "and every round leans toward the port"
        );
    }

    #[test]
    fn the_alliance_promotes_a_pilot_who_keeps_flying() {
        assert_eq!(Rank::of_level(1), Rank::FlightCadet);
        assert_eq!(Rank::of_level(7), Rank::Lieutenant);
        assert_eq!(Rank::of_level(30), Rank::General);
        assert!(Rank::General > Rank::FlightCadet, "and rank has an order");
        let mut g = flying();
        g.level = 12;
        assert_eq!(
            g.rank(),
            Rank::Captain,
            "the pilot wears what he has earned"
        );
    }

    #[test]
    fn the_squadron_calls_it_out_over_the_radio() {
        let mut g = flying();
        g.chatter.clear();
        g.shield = 1;
        g.damage_ship(1);
        assert!(!g.chatter.is_empty(), "losing the deflectors is called out");
        let first = g.chatter[0].line.clone();
        g.say(&first);
        assert_eq!(g.chatter.len(), 1, "the same line is not repeated");
        for _ in 0..CHATTER_TICKS {
            g.tick_timers();
        }
        assert!(g.chatter.is_empty(), "and traffic ages off the display");
    }

    #[test]
    fn an_interdictor_drags_twice_as_hard_as_a_tractor_beam() {
        let mut g = flying();
        g.node.kind = NodeKind::Capital;
        g.capital = Some(Capital::new(CapitalKind::Interdictor, 0, 0));
        {
            let cap = g.capital.as_mut().unwrap();
            cap.pos.1 = W - 14;
            cap.tick = 1;
            for part in cap.parts.iter_mut() {
                if part.kind != Emplacement::GravityProjector {
                    part.hp = 0;
                }
            }
        }
        g.ship.1 = 10;
        g.advance_capital();
        assert!(
            g.ship.1 >= 12,
            "the wells haul the hull two columns at a time, not one"
        );
    }

    #[test]
    fn the_ace_flies_his_own_pattern() {
        let ace = Boss::new(BossKind::AceTie, 60);
        assert!(ace.parts.is_empty(), "one fighter, no emplacements");
        assert_eq!(ace.kind.core_half(), 1, "and a fighter-sized target");
        assert!(
            ace.speed() >= 2,
            "he moves faster than anything else in the sky"
        );
        let bosses: Vec<BossKind> = (1..=5).map(|n| BossKind::of_wave(n * BOSS_EVERY)).collect();
        assert!(
            bosses.contains(&BossKind::AceTie),
            "and he holds boss systems"
        );
    }

    #[test]
    fn every_fighter_and_every_tie_is_drawn_differently() {
        let hulls: std::collections::HashSet<[&str; 2]> =
            ShipClass::ALL.iter().map(|c| c.sprite()).collect();
        assert_eq!(
            hulls.len(),
            ShipClass::ALL.len(),
            "no two fighters look alike"
        );
        for class in ShipClass::ALL {
            for row in class.sprite() {
                assert_eq!(
                    row.chars().count(),
                    3,
                    "{} is three cells wide",
                    class.name()
                );
            }
        }
        let ties = [
            EnemyKind::TieFighter,
            EnemyKind::TieInterceptor,
            EnemyKind::TieBomber,
            EnemyKind::TieDefender,
            EnemyKind::TieAdvanced,
        ];
        let shapes: std::collections::HashSet<&str> = ties.iter().map(|k| k.sprite()).collect();
        assert_eq!(shapes.len(), ties.len(), "and no two TIEs do either");
    }
}

#[cfg(test)]
mod campaign_tests {
    use super::tests::flying;
    use super::*;

    fn on_mission(index: usize) -> Game {
        let mut g = Game::new(6);
        g.start_campaign(ShipClass::XWing, Difficulty::Normal);
        g.fly_mission(index);
        g
    }

    #[test]
    fn the_campaign_flies_the_war_in_order() {
        let mut g = Game::new(6);
        g.start_campaign(ShipClass::XWing, Difficulty::Normal);
        assert_eq!(
            g.status,
            Status::Playing,
            "the first mission starts at once"
        );
        assert_eq!(
            g.mission.unwrap().name,
            Mission::CAMPAIGN[0].name,
            "with the first mission of the war"
        );
        let names: Vec<&str> = Mission::CAMPAIGN.iter().map(|m| m.name).collect();
        assert!(names.contains(&"Battle of Yavin"));
        assert!(names.contains(&"Battle of Hoth"));
        assert!(names.contains(&"Battle of Endor"));
        g.fly_mission(Mission::CAMPAIGN.len());
        assert!(g.mission.is_none(), "past the last one the war is over");
        assert_eq!(g.status, Status::Ceremony, "and the ceremony closes it");
    }

    #[test]
    fn yavin_is_a_trench_run_at_a_station() {
        let g = on_mission(3);
        assert_eq!(g.mission.unwrap().name, "Battle of Yavin");
        assert_eq!(g.objective, Objective::CoreRun, "one shot down the shaft");
        assert_eq!(g.node.terrain, TerrainKind::Trench);
        assert_eq!(
            g.capital.as_ref().map(|c| c.kind),
            Some(CapitalKind::DeathStar),
            "and a station to run"
        );
    }

    #[test]
    fn hoth_puts_walkers_on_the_deck_and_only_cables_stop_them() {
        let mut g = on_mission(4);
        assert_eq!(g.objective, Objective::Walkers { count: 3 });
        assert_eq!(g.walkers.len(), 3, "three of them on the ridge");
        let pos = g.walkers[0].pos;
        let hp = g.walkers[0].hp;
        g.hit_targets(&Shot::bolt(pos, 0, 40));
        assert_eq!(g.walkers[0].hp, hp, "cannons do not mark the armour");
        assert!(g.hit_targets(&Shot::cable(pos, 0)), "the cable catches");
        assert_eq!(g.walkers[0].wraps, 1, "one turn round the legs");
        assert!(!g.walkers[0].down, "one turn is not enough");
        g.hit_targets(&Shot::cable(pos, 0));
        assert!(g.walkers[0].down, "the second turn puts it over");
        for walker in g.walkers.iter_mut() {
            walker.down = true;
        }
        g.check_end();
        assert_eq!(g.status, Status::WaveClear, "and that is the mission");
    }

    #[test]
    fn the_evacuation_is_won_by_transports_that_get_away() {
        let mut g = on_mission(5);
        assert!(matches!(g.objective, Objective::Escort { .. }));
        assert!(g.transports.len() >= 4, "the convoy is on the deck");
        for transport in g.transports.iter_mut() {
            transport.away = true;
        }
        let score = g.score;
        g.check_end();
        assert_eq!(g.status, Status::WaveClear, "the convoy is through");
        assert!(g.score > score, "and it pays");
    }

    #[test]
    fn a_transport_under_fire_can_be_lost() {
        let mut g = on_mission(5);
        let pos = g.transports[0].pos;
        g.transports[0].hp = 2;
        g.enemy_shots = vec![Shot::enemy((pos.0, pos.1 + 1), 0, 1)];
        g.tick = 3;
        g.advance_transports();
        assert!(g.transports[0].hp <= 0, "the shot goes home");
        assert!(
            g.chatter.iter().any(|c| c.line.contains("transports")),
            "and the squadron hears about it"
        );
    }

    #[test]
    fn the_kessel_run_is_a_hold_out() {
        let mut g = on_mission(1);
        assert!(matches!(g.objective, Objective::Survive { .. }));
        assert!(g.objective_ticks > 0, "with a clock on it");
        g.objective_ticks = 0;
        g.check_end();
        assert_eq!(g.status, Status::WaveClear, "outrunning it is the mission");
    }

    #[test]
    fn every_system_hangs_over_a_world() {
        for sector in Sector::ALL {
            let planet = Planet::of_sector(sector);
            assert!(!planet.name().is_empty());
        }
        assert_eq!(Planet::of_sector(Sector::AsteroidBelt), Planet::Hoth);
        assert_eq!(Planet::of_sector(Sector::SolarCorona), Planet::Tatooine);
        assert!(
            Planet::Hoth.surface(),
            "some of them are fought on the deck"
        );
        assert!(!Planet::DeepSpace.surface(), "and some are not");
        let mut g = flying();
        g.node.sector = Sector::CometTrail;
        g.spawn_wave();
        assert_eq!(g.planet, Planet::Bespin, "the world follows the sector");
    }

    #[test]
    fn every_mission_in_the_campaign_can_be_flown() {
        for index in 0..Mission::CAMPAIGN.len() {
            let mut g = on_mission(index);
            let mission = g.mission.expect("the mission is loaded");
            assert_eq!(g.sector, mission.sector, "{} is in place", mission.name);
            for i in 0..600 {
                if g.status != Status::Playing {
                    break;
                }
                if i % 3 == 0 {
                    g.fire();
                }
                if i % 29 == 0 {
                    g.fire_missiles();
                }
                g.move_ship(if i % 7 < 3 { 1 } else { -1 }, 0);
                g.step();
            }
            assert!(
                (1..W - 1).contains(&g.ship.1),
                "{} flies without losing the hull off the court",
                mission.name
            );
        }
    }
}

#[cfg(test)]
mod deck_tests {
    use super::tests::flying;
    use super::*;

    fn docked() -> Game {
        let mut g = flying();
        g.status = Status::Hangar;
        g.credits = 40_000;
        g.deck = Deck::new(g.squad.len());
        g
    }

    fn stand_at(g: &mut Game, spot: DeckSpot) {
        let pos = g.deck.spot_at(spot).expect("the deck has one");
        g.deck.pilot = pos;
    }

    #[test]
    fn the_pilot_walks_the_deck_and_stays_on_it() {
        let mut g = docked();
        let start = g.deck.pilot;
        g.walk(1, 0);
        assert_eq!(g.deck.pilot.1, start.1 + 1, "a step to starboard");
        for _ in 0..200 {
            g.walk(-1, -1);
        }
        assert_eq!(g.deck.pilot, (1, 1), "and the bulkheads stop him");
        g.status = Status::Playing;
        let held = g.deck.pilot;
        g.walk(1, 1);
        assert_eq!(g.deck.pilot, held, "nobody walks the deck mid-sortie");
    }

    #[test]
    fn standing_at_a_bay_boards_that_fighter() {
        let mut g = docked();
        let hull = g
            .shop_lines()
            .into_iter()
            .find(|l| l.entry == ShopEntry::Consumable(Stock::Hull))
            .expect("the yard sells fighters");
        g.shop_open = true;
        assert!(g.buy(hull.key));
        g.shop_open = false;
        g.deck = Deck::new(g.squad.len());
        stand_at(&mut g, DeckSpot::Bay(1));
        assert_eq!(g.at_hand(), Some(DeckSpot::Bay(1)), "he is at the bay");
        assert_eq!(g.interact(), Some(DeckAction::Boarded(1)), "and climbs in");
        assert_eq!(g.active, 1, "that fighter is his now");
    }

    #[test]
    fn the_stations_do_what_they_say() {
        let mut g = docked();
        stand_at(&mut g, DeckSpot::Quartermaster);
        assert_eq!(g.interact(), Some(DeckAction::OpenedShop));
        assert!(g.shop_open, "the terminal is up");
        assert_eq!(g.interact(), None, "and Enter shuts it again");
        assert!(!g.shop_open);

        g.shield = 0;
        g.squad.push(Wing::new("Red Two", ShipClass::AWing));
        g.squad[1].alive = false;
        g.deck = Deck::new(g.squad.len());
        stand_at(&mut g, DeckSpot::AstromechPit);
        assert_eq!(g.interact(), Some(DeckAction::Repaired));
        assert_eq!(g.shield, g.max_shield, "the droids top the shields up");
        assert!(g.squad[1].alive, "and put the wing back together");

        stand_at(&mut g, DeckSpot::BriefingTable);
        assert_eq!(g.interact(), Some(DeckAction::ReadBriefing));
        assert!(!g.chatter.is_empty(), "the briefing is read out");

        stand_at(&mut g, DeckSpot::Navicomputer);
        assert_eq!(g.interact(), Some(DeckAction::OpenedChart));
        assert_eq!(g.status, Status::Chart, "and the chart comes up");
    }

    #[test]
    fn the_launch_pad_flies_the_next_mission() {
        let mut g = Game::new(8);
        g.start_campaign(ShipClass::XWing, Difficulty::Normal);
        g.status = Status::Hangar;
        g.deck = Deck::new(g.squad.len());
        let at = g.campaign_at;
        stand_at(&mut g, DeckSpot::LaunchPad);
        assert_eq!(g.interact(), Some(DeckAction::Launched));
        assert_eq!(g.campaign_at, at + 1, "the next mission is on");
        assert_eq!(g.status, Status::Playing);
    }

    #[test]
    fn a_squadron_can_be_six_fighters_strong() {
        let mut g = docked();
        while g.squad.len() < MAX_SQUAD {
            let hull = g
                .shop_lines()
                .into_iter()
                .find(|l| l.entry == ShopEntry::Consumable(Stock::Hull))
                .expect("the yard still has one");
            assert!(hull.available, "and it is for sale");
            g.shop_open = true;
            assert!(g.buy(hull.key));
            g.shop_open = false;
        }
        assert_eq!(g.squad.len(), 6, "six fighters on the line");
        assert!(
            !g.shop_lines()
                .iter()
                .any(|l| l.entry == ShopEntry::Consumable(Stock::Hull) && l.available),
            "and that is the last of them"
        );
        g.status = Status::Playing;
        assert_eq!(g.wing_cells().len(), 5, "five of them fly on your wing");
        assert_eq!(
            Deck::new(g.squad.len())
                .spots
                .iter()
                .filter(|(s, _)| matches!(s, DeckSpot::Bay(_)))
                .count(),
            6,
            "and the deck has a bay for each"
        );
    }

    #[test]
    fn the_wing_flies_the_formation_it_is_given() {
        let mut g = docked();
        for _ in 0..3 {
            g.squad.push(Wing::new("Red Two", ShipClass::AWing));
        }
        g.status = Status::Playing;
        // Fly up off the deck line so the wing has room astern.
        g.ship.0 = SHIP_TOP;
        g.wing_formation = WingFormation::LineAbreast;
        let abreast = g.wing_cells();
        assert!(
            abreast.iter().all(|(_, pos)| pos.0 == abreast[0].1 .0),
            "line abreast flies level"
        );
        g.wing_formation = WingFormation::Trail;
        let trail = g.wing_cells();
        assert!(
            trail.windows(2).all(|w| w[0].1 .0 < w[1].1 .0),
            "trail is line astern"
        );
        assert!(
            trail.iter().all(|(_, pos)| pos.1 == g.ship.1),
            "and stacked right behind the leader"
        );
        g.wing_formation = WingFormation::Echelon;
        assert!(
            g.wing_cells().iter().all(|(_, pos)| pos.1 <= g.ship.1),
            "echelon stacks off one wing"
        );
        let shape = g.cycle_formation();
        assert_ne!(shape, WingFormation::Echelon, "and the order changes it");
    }
}

#[cfg(test)]
mod frontier_tests {
    use super::tests::flying;
    use super::*;

    #[test]
    fn a_tuned_hyperdrive_reaches_further_across_the_chart() {
        let mut g = flying();
        g.status = Status::Chart;
        assert_eq!(g.jump_range(), 1, "stock, one lane at a time");
        let near = g.map.reachable_within(1).len();
        g.loadout.tiers[Part::Hyperdrive as usize] = 4;
        assert_eq!(g.jump_range(), 3, "tuned, three");
        assert!(
            g.map.reachable_within(3).len() > near,
            "and the chart opens up"
        );
        let far = g.map.reachable_within(3);
        g.map.cursor = *far.last().unwrap();
        assert!(g.jump(), "a long jump is flown in one go");
    }

    #[test]
    fn gravity_wells_hold_the_squadron_to_one_lane() {
        let mut g = flying();
        g.status = Status::Chart;
        g.loadout.tiers[Part::Hyperdrive as usize] = 4;
        let out = g.map.reachable()[0];
        g.map.nodes[out].kind = NodeKind::Capital;
        g.map.nodes[out].cleared = false;
        g.map.nodes[out].terrain = TerrainKind::Open;
        assert!(g.interdicted(), "there is a well one lane out");
        assert_eq!(g.jump_range(), 1, "and nothing jumps past it");
        g.map.nodes[out].cleared = true;
        assert!(!g.interdicted(), "clear it and the lanes open");
        assert_eq!(g.jump_range(), 3);
    }

    #[test]
    fn the_fleet_comes_out_for_the_big_ships_and_fires_with_you() {
        let mut g = flying();
        g.node.kind = NodeKind::Capital;
        g.node.region = Region::Core;
        g.node.terrain = TerrainKind::Open;
        g.spawn_wave();
        let ally = g.ally.as_ref().expect("the cruiser is with us");
        assert_eq!(ally.kind, CapitalKind::MonCalamari);
        for part in g.ally.as_mut().unwrap().parts.iter_mut() {
            part.cooldown = 0;
        }
        g.shots.clear();
        g.advance_ally();
        assert!(!g.shots.is_empty(), "her batteries fire up the court");
        assert!(
            g.shots.iter().all(|s| s.speed < 0),
            "on our side of the fight"
        );
    }

    #[test]
    fn imperial_fire_that_reaches_the_cruiser_hits_her_instead_of_us() {
        let mut g = flying();
        let mut ally = Capital::new(CapitalKind::MonCalamari, 0, 0);
        ally.pos = (H - 5, W / 3);
        let cell = (ally.pos.0, ally.pos.1);
        let hp = ally.hp;
        g.ally = Some(ally);
        g.enemy_shots = vec![Shot::enemy((cell.0 - 1, cell.1), 0, 1)];
        let shield = g.shield;
        g.advance_enemy_shots();
        assert!(g.ally.as_ref().unwrap().hp < hp, "the cruiser takes it");
        assert_eq!(g.shield, shield, "and we do not");
    }

    #[test]
    fn the_squadron_can_put_down_on_a_world_and_walk_it() {
        let mut g = flying();
        g.status = Status::Chart;
        let at = g.map.cursor;
        g.map.nodes[at].sector = Sector::AsteroidBelt;
        assert!(g.land(), "there is a world down there");
        assert_eq!(g.status, Status::Surface);
        assert_eq!(g.planet, Planet::Hoth);
        assert!(
            g.deck.spot_at(DeckSpot::ParkedShip).is_some(),
            "the fighter is parked where we left it"
        );
        let start = g.deck.pilot;
        g.walk(1, 0);
        assert_ne!(g.deck.pilot, start, "and the pilot walks about");
        g.deck.pilot = g.deck.spot_at(DeckSpot::ParkedShip).unwrap();
        assert_eq!(g.interact(), Some(DeckAction::LiftedOff));
        assert_eq!(g.status, Status::Chart, "and lifts off again");
    }

    #[test]
    fn deep_space_has_nothing_to_land_on() {
        let mut g = flying();
        g.status = Status::Chart;
        let at = g.map.cursor;
        g.map.nodes[at].sector = Sector::OpenSpace;
        assert!(!g.land(), "you cannot walk about in vacuum");
        assert_eq!(g.status, Status::Chart);
    }

    #[test]
    fn what_is_down_there_is_worth_the_walk() {
        let mut g = flying();
        g.status = Status::Surface;
        g.deck = Deck::surface(Planet::Tatooine, 5);
        g.credits = 0;
        if g.deck.spot_at(DeckSpot::SurfaceWreck).is_some() {
            g.deck.pilot = g.deck.spot_at(DeckSpot::SurfaceWreck).unwrap();
            assert_eq!(g.interact(), Some(DeckAction::Stripped));
            assert!(g.credits > 0, "a wreck is worth stripping");
        }
        g.deck = Deck::surface(Planet::Yavin, 9);
        if let Some(pos) = g.deck.spot_at(DeckSpot::Ruins) {
            g.deck.pilot = pos;
            g.force = 0;
            assert_eq!(g.interact(), Some(DeckAction::Explored));
            assert_eq!(g.force, FORCE_MAX, "the ruins are strong with it");
        }
        g.deck = Deck::surface(Planet::Tatooine, 3);
        if let Some(pos) = g.deck.spot_at(DeckSpot::Cantina) {
            g.deck.pilot = pos;
            let charted = g.map.charted();
            assert_eq!(g.interact(), Some(DeckAction::Drank));
            assert!(g.map.charted() >= charted, "and the cantina talks");
        }
    }

    #[test]
    fn an_escape_pod_puts_a_pilot_back_in_the_air() {
        let mut g = flying();
        g.squad.push(Wing::new("Red Two", ShipClass::AWing));
        g.squad[1].alive = false;
        g.collect(PowerKind::EscapePod);
        assert!(g.squad[1].alive, "the pilot is back");
        g.collect(PowerKind::EscapePod);
        assert!(g.lives >= 4, "and a spare pilot is a spare life");
    }

    #[test]
    fn the_forest_is_thick_enough_to_matter() {
        let forest = Terrain::new(TerrainKind::Forest, 4);
        let trunks: usize = forest.rows.iter().map(|r| r.pillars.len()).sum();
        assert!(trunks > 0, "there are trunks in it");
        assert!(
            TerrainKind::Forest.destructible(),
            "and a cannon will clear one"
        );
    }

    #[test]
    fn the_war_ends_in_a_ceremony() {
        let mut g = Game::new(3);
        g.start_campaign(ShipClass::XWing, Difficulty::Normal);
        g.fly_mission(Mission::CAMPAIGN.len());
        assert_eq!(g.status, Status::Ceremony, "the war is over");
        assert!(g.mission.is_none());
    }
}

#[cfg(test)]
mod pov_tests {
    use super::tests::flying;
    use super::*;

    #[test]
    fn the_pilot_turns_on_the_spot_and_walks_where_he_looks() {
        let mut deck = Deck::new(2);
        deck.pilot = (8, 20);
        deck.facing = (-1, 0);
        deck.turn(1);
        assert_eq!(deck.facing, (0, 1), "a turn to starboard");
        deck.step(1);
        assert_eq!(deck.pilot, (8, 21), "and a pace that way");
        deck.turn(-1);
        deck.turn(-1);
        assert_eq!(
            deck.facing,
            (0, -1),
            "two turns to port faces the other way"
        );
        deck.step(2);
        assert_eq!(deck.pilot, (8, 19), "walking two paces along it");
        deck.step(-1);
        assert_eq!(deck.pilot, (8, 20), "and one back off it");
    }

    #[test]
    fn what_is_in_front_of_you_is_what_you_can_use() {
        let mut deck = Deck::new(3);
        let bay = deck.spot_at(DeckSpot::Bay(1)).expect("the bay is there");
        // Stand well back from it, looking the other way.
        deck.pilot = (bay.0 + 4, bay.1);
        deck.facing = (1, 0);
        assert_ne!(
            deck.at_hand(),
            Some(DeckSpot::Bay(1)),
            "with your back to it you cannot reach it"
        );
        deck.turn(1);
        deck.turn(1);
        assert_eq!(deck.facing, (-1, 0), "turn about");
        assert_eq!(
            deck.at_hand(),
            Some(DeckSpot::Bay(1)),
            "and it is in front of you"
        );
    }

    #[test]
    fn walking_up_to_a_fighter_shows_that_fighter() {
        let mut g = flying();
        g.status = Status::Hangar;
        g.squad.push(Wing::new("Red Two", ShipClass::YWing));
        g.deck = Deck::new(g.squad.len());
        let bay = g.deck.spot_at(DeckSpot::Bay(1)).unwrap();
        g.deck.pilot = (bay.0 + 3, bay.1);
        g.deck.facing = (-1, 0);
        assert_eq!(g.at_hand(), Some(DeckSpot::Bay(1)), "it is dead ahead");
        assert_eq!(g.interact(), Some(DeckAction::Boarded(1)), "and boardable");
        assert_eq!(g.class, ShipClass::YWing, "the hull you walked up to");
    }

    #[test]
    fn the_deck_can_be_walked_end_to_end_in_first_person() {
        let mut g = flying();
        g.status = Status::Surface;
        g.deck = Deck::surface(Planet::Tatooine, 11);
        let mut seen = std::collections::HashSet::new();
        for turn in 0..4 {
            g.turn(1);
            for _ in 0..Deck::WIDTH {
                g.step_ahead(1);
                if let Some(spot) = g.at_hand() {
                    seen.insert(format!("{spot:?}"));
                }
            }
            assert!(
                (1..Deck::HEIGHT - 1).contains(&g.deck.pilot.0)
                    && (1..Deck::WIDTH - 1).contains(&g.deck.pilot.1),
                "the pilot stays on the ground after turn {turn}"
            );
        }
        assert!(!seen.is_empty(), "and walks up to things while he is at it");
    }
}

#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn every_fighter_has_a_model_you_can_tell_apart() {
        let models: std::collections::HashSet<[&str; 5]> =
            ShipClass::ALL.iter().map(|c| c.model()).collect();
        assert_eq!(
            models.len(),
            ShipClass::ALL.len(),
            "no two fighters are built the same"
        );
        for class in ShipClass::ALL {
            for row in class.model() {
                assert_eq!(
                    row.chars().count(),
                    7,
                    "{} is seven cells across in its own model",
                    class.name()
                );
            }
            assert!(
                class.model().iter().any(|row| row.contains('█')),
                "{} has hull in it",
                class.name()
            );
        }
    }

    #[test]
    fn hull_shades_out_with_distance() {
        assert_eq!(depth_shade(1), "█", "close up it is solid");
        assert_eq!(depth_shade(5), "▓");
        assert_eq!(depth_shade(10), "▒");
        assert_eq!(depth_shade(20), "░", "and far off it is barely there");
    }
}

#[cfg(test)]
mod ground_tests {
    use super::tests::flying;
    use super::*;

    fn planetside(planet: Planet) -> Game {
        let mut g = flying();
        g.status = Status::Surface;
        g.planet = planet;
        g.deck = Deck::surface(planet, 21);
        g
    }

    #[test]
    fn a_world_is_big_and_has_things_on_it() {
        let g = planetside(Planet::Coruscant);
        const _: () = assert!(Deck::WIDTH >= 120 && Deck::HEIGHT >= 40, "worlds are large");
        assert!(!g.deck.cover.is_empty(), "with blocks to hide behind");
        assert!(!g.deck.troopers.is_empty(), "and a patrol on them");
        let city = g.deck.cover.iter().filter(|c| c.tall).count();
        assert!(city > 0, "a city world is built up");
        let wild = planetside(Planet::Hoth);
        assert!(
            wild.deck.cover.iter().all(|c| !c.tall),
            "and the wilderness is not"
        );
    }

    #[test]
    fn cover_is_solid_and_stops_a_pilot_walking_through_it() {
        let mut g = planetside(Planet::Bespin);
        let block = g.deck.cover[0];
        let inside = (block.pos.0, block.pos.1);
        assert!(g.deck.blocked(inside.0, inside.1), "the block is solid");
        g.deck.pilot = (inside.0, inside.1 - 1);
        g.deck.facing = (0, 1);
        let stood = g.deck.pilot;
        g.step_ahead(1);
        assert_eq!(g.deck.pilot, stood, "and you cannot walk into it");
    }

    #[test]
    fn the_blaster_fires_where_the_pilot_looks_and_drops_a_trooper() {
        let mut g = planetside(Planet::Tatooine);
        g.deck.troopers.clear();
        g.deck.pilot = (20, 20);
        g.deck.facing = (0, 1);
        g.deck.cover.clear();
        g.deck
            .troopers
            .push(Trooper::new(GroundKind::Trooper, (20, 26), 99));
        assert!(g.shoot(), "the sidearm fires");
        assert!(!g.deck.bolts.is_empty(), "a bolt is in the air");
        assert!(!g.shoot(), "and it cycles before the next one");
        for tick in 0..4 {
            g.deck.skirmish(tick);
        }
        assert_eq!(g.deck.troopers[0].hp, 1, "one bolt takes two off him");
        assert!(g.shoot(), "the sidearm has cycled");
        for tick in 0..6 {
            g.deck.skirmish(tick);
        }
        assert!(g.deck.troopers.is_empty(), "and the second puts him down");
    }

    #[test]
    fn a_patrol_closes_and_shoots_back() {
        let mut g = planetside(Planet::Jakku);
        g.deck.cover.clear();
        g.deck.bolts.clear();
        g.deck.pilot = (20, 20);
        g.deck.troopers = vec![Trooper::new(GroundKind::Trooper, (20, 30), 0)];
        let start = g.deck.troopers[0].pos;
        let mut fired = false;
        for tick in 0..12 {
            g.deck.skirmish(tick);
            fired |= g.deck.bolts.iter().any(|b| !b.friendly);
        }
        assert!(fired, "they shoot back");
        assert_ne!(g.deck.troopers[0].pos, start, "and they close the range");
    }

    #[test]
    fn taking_too_much_of_it_ends_the_walkabout() {
        let mut g = planetside(Planet::Mustafar);
        g.deck.troopers.clear();
        g.deck.cover.clear();
        g.deck.health = 1;
        g.deck.bolts = vec![Bolt {
            pos: (g.deck.pilot.0 - 1, g.deck.pilot.1),
            dir: (1, 0),
            friendly: false,
            life: 4,
        }];
        let lives = g.lives;
        for _ in 0..4 {
            g.ground_tick();
        }
        assert_eq!(g.status, Status::Chart, "he is carried back to the ship");
        assert_eq!(g.lives, lives - 1, "and it costs");
        assert_eq!(g.deck.health, PILOT_HEALTH, "patched up for next time");
    }

    #[test]
    fn nothing_shoots_on_the_hangar_deck() {
        let mut g = flying();
        g.status = Status::Hangar;
        g.deck = Deck::new(2);
        assert!(g.deck.troopers.is_empty(), "the yard is friendly");
        assert!(g.deck.cover.is_empty(), "and clear to walk");
        g.ground_tick();
        assert_eq!(g.status, Status::Hangar, "and the tick does nothing there");
    }
}

#[cfg(test)]
mod raster_tests {
    use super::*;

    fn camera(w: i16, h: i16) -> Camera {
        Camera {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            yaw: 0.0,
            focal: w as f32 * 0.6,
            w: w as f32,
            h: h as f32,
        }
    }

    fn filled(canvas: &Canvas) -> usize {
        canvas.cells.iter().filter(|cell| cell.is_some()).count()
    }

    #[test]
    fn a_face_square_to_the_light_is_brighter_than_one_edge_on() {
        let lit = face_light([0.0, 1.0, 0.0], 1.0);
        let dark = face_light([0.0, -1.0, 0.0], 1.0);
        assert!(lit > dark, "the top of a hull catches the light");
        assert!(dark > 0.0, "and the underside is not pitch black");
        assert!(
            face_light([0.0, 1.0, 0.0], 40.0) < lit,
            "distance eats the light"
        );
        assert_eq!(ramp_glyph(1.0), '█', "full brightness is solid");
        assert_eq!(ramp_glyph(0.0), ' ', "and nothing is nothing");
    }

    #[test]
    fn a_box_rasterises_into_a_solid_block_of_cells() {
        let cam = camera(60, 24);
        let mut canvas = Canvas::new(60, 24);
        let solid = Solid::new([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], '█');
        draw_solid(
            &mut canvas,
            &cam,
            &solid,
            [0.0, 0.0, 8.0],
            1.0,
            Default::default(),
        );
        let painted = filled(&canvas);
        assert!(painted > 40, "the face fills, it does not dot: {painted}");
    }

    #[test]
    fn nearer_geometry_hides_what_is_behind_it() {
        let cam = camera(60, 24);
        let mut canvas = Canvas::new(60, 24);
        let far = Solid::new([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], '█');
        draw_solid(
            &mut canvas,
            &cam,
            &far,
            [0.0, 0.0, 20.0],
            1.0,
            Default::default(),
        );
        let behind: Vec<Option<f32>> = canvas
            .cells
            .iter()
            .map(|cell| cell.map(|c| c.depth))
            .collect();
        let near = Solid::new([0.0, 0.0, 0.0], [1.5, 1.5, 1.5], '█');
        draw_solid(
            &mut canvas,
            &cam,
            &near,
            [0.0, 0.0, 6.0],
            1.0,
            Default::default(),
        );
        let mut covered = 0;
        for (i, cell) in canvas.cells.iter().enumerate() {
            if let (Some(now), Some(Some(was))) = (cell, behind.get(i)) {
                if now.depth < *was {
                    covered += 1;
                }
            }
        }
        assert!(covered > 0, "the near hull takes those cells over");
        assert!(
            canvas.cells.iter().flatten().all(|cell| cell.depth > 0.0),
            "and nothing ends up in front of the lens"
        );
    }

    #[test]
    fn a_box_behind_the_camera_is_not_drawn() {
        let cam = camera(60, 24);
        let mut canvas = Canvas::new(60, 24);
        let solid = Solid::new([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], '█');
        draw_solid(
            &mut canvas,
            &cam,
            &solid,
            [0.0, 0.0, -10.0],
            1.0,
            Default::default(),
        );
        assert_eq!(filled(&canvas), 0, "what is behind you is behind you");
    }

    #[test]
    fn the_canopy_frames_the_glass() {
        let mut canvas = Canvas::new(40, 16);
        draw_canopy(&mut canvas, Default::default());
        let corner = canvas.cells[0].expect("the frame is drawn");
        assert!(corner.depth < 0.1, "and it sits in front of everything");
        assert!(filled(&canvas) > 40, "with struts and a coaming on it");
    }

    #[test]
    fn a_planet_disc_is_wider_than_it_is_tall() {
        let mut canvas = Canvas::new(60, 24);
        draw_planet_disc(&mut canvas, (12, 30), 6, '▓', Default::default());
        let cells: Vec<(i16, i16)> = (0..canvas.h)
            .flat_map(|y| (0..canvas.w).map(move |x| (x, y)))
            .filter(|(x, y)| canvas.cells[(y * canvas.w + x) as usize].is_some())
            .collect();
        let width =
            cells.iter().map(|c| c.0).max().unwrap() - cells.iter().map(|c| c.0).min().unwrap();
        let height =
            cells.iter().map(|c| c.1).max().unwrap() - cells.iter().map(|c| c.1).min().unwrap();
        assert!(
            width > height,
            "terminal cells are tall, so a round world is drawn wide: {width}x{height}"
        );
    }

    #[test]
    fn every_hull_in_the_game_has_geometry_to_draw() {
        for class in ShipClass::ALL {
            assert!(!class.solid().is_empty(), "{} is built", class.name());
        }
        for kind in [
            EnemyKind::TieFighter,
            EnemyKind::TieInterceptor,
            EnemyKind::TieBomber,
            EnemyKind::Gunboat,
            EnemyKind::GunPlatform,
            EnemyKind::BuzzDroid,
        ] {
            assert!(!kind.solid().is_empty(), "{} is built", kind.glyph());
        }
    }
}

#[cfg(test)]
mod frame_tests {
    use super::*;

    /// The frame as text, which is how these were looked at while they were
    /// being built.
    fn dump(canvas: &Canvas) -> String {
        let mut out = String::new();
        for y in 0..canvas.h {
            for x in 0..canvas.w {
                let cell = canvas.cells[(y * canvas.w + x) as usize];
                out.push(cell.map_or(' ', |c| c.glyph));
            }
            out.push('\n');
        }
        out
    }

    /// How much of the frame is drawn on, and how many shading levels it uses.
    fn coverage(canvas: &Canvas) -> (f32, usize) {
        let drawn = canvas.cells.iter().flatten().count();
        let shades: std::collections::HashSet<char> = canvas
            .cells
            .iter()
            .flatten()
            .map(|cell| cell.glyph)
            .filter(|glyph| RAMP.contains(glyph))
            .collect();
        (drawn as f32 / canvas.cells.len() as f32, shades.len())
    }

    #[test]
    fn a_cockpit_frame_is_a_scene_and_not_an_empty_box() {
        let (w, h) = (96i16, 30i16);
        let mut canvas = Canvas::new(w, h);
        let cam = pilot_camera((20, 38), w, h);
        let style = zmax_view::graphics::Style::default();
        draw_horizon(&mut canvas, &cam, '·', '·', style, style);
        draw_planet_disc(&mut canvas, (h / 4, w / 4), h / 5, '▓', style);
        for (row, col) in [(14, 30), (10, 44), (6, 36), (16, 50)] {
            draw_model(
                &mut canvas,
                &cam,
                EnemyKind::TieFighter.solid(),
                [col as f32, 0.0, row as f32],
                HULL_SCALE,
                style,
            );
        }
        draw_model(
            &mut canvas,
            &cam,
            ShipClass::XWing.solid(),
            [44.0, -0.6, 21.0],
            HULL_SCALE,
            style,
        );
        for dr in 0..4 {
            let slab = Solid::new([0.0, 0.0, 0.0], [8.0, 0.6, 0.5], '█');
            draw_solid(
                &mut canvas,
                &cam,
                &slab,
                [38.0, 1.5, (2 + dr) as f32],
                1.0,
                style,
            );
        }
        draw_canopy(&mut canvas, style);
        let frame = dump(&canvas);
        let (filled, shades) = coverage(&canvas);
        assert!(
            filled > 0.4,
            "the glass is mostly drawn on, not empty: {filled}
{frame}"
        );
        assert!(
            shades >= 3,
            "and the hulls are shaded, not one flat tone: {shades}
{frame}"
        );
        assert!(
            frame.lines().next().is_some_and(|top| top.contains('═')),
            "with the canopy round it
{frame}"
        );
        assert!(
            frame.contains('▓') && frame.contains('░'),
            "a world in the sky and hulls in front of it
{frame}"
        );
    }

    #[test]
    fn a_hangar_frame_puts_the_fighters_in_front_of_you() {
        let (w, h) = (96i16, 30i16);
        let mut deck = Deck::new(3);
        deck.pilot = (14, 24);
        deck.facing = (-1, 0);
        let mut canvas = Canvas::new(w, h);
        let cam = walker_camera(&deck, w, h);
        let style = zmax_view::graphics::Style::default();
        draw_horizon(&mut canvas, &cam, '▒', '·', style, style);
        draw_ground(&mut canvas, &cam, 0.0, 30.0, 2.0, '·', style);
        for (spot, pos) in &deck.spots {
            let origin = [pos.1 as f32, 0.9, pos.0 as f32];
            match spot {
                DeckSpot::Bay(i) => draw_model(
                    &mut canvas,
                    &cam,
                    ShipClass::ALL[i % ShipClass::ALL.len()].solid(),
                    origin,
                    HULL_SCALE,
                    style,
                ),
                _ => {
                    let body = Solid::new([0.0, -0.3, 0.0], [0.8, 0.5, 0.8], '█');
                    draw_solid(&mut canvas, &cam, &body, origin, 1.0, style);
                }
            }
        }
        let frame = dump(&canvas);
        let (filled, shades) = coverage(&canvas);
        assert!(
            filled > 0.25,
            "the deck is drawn, not blank: {filled}
{frame}"
        );
        assert!(
            shades >= 3,
            "with lit and shaded faces: {shades}
{frame}"
        );
        // A fighter parked ahead should be a solid mass of hull somewhere in
        // the middle of the frame, not a scattering of marks.
        let middle: Vec<&str> = frame.lines().skip(10).take(10).collect();
        let solid = middle
            .iter()
            .map(|row| {
                row.chars()
                    .filter(|glyph| matches!(glyph, '█' | '▓' | '▒'))
                    .count()
            })
            .max()
            .unwrap_or(0);
        assert!(
            solid > 8,
            "the fighter in front of you fills the view: {solid}
{frame}"
        );
    }
}

#[cfg(test)]
mod sabre_tests {
    use super::tests::flying;
    use super::*;

    fn planetside() -> Game {
        let mut g = flying();
        g.status = Status::Surface;
        g.planet = Planet::Endor;
        g.deck = Deck::surface(Planet::Endor, 31);
        g.deck.cover.clear();
        g.deck.troopers.clear();
        g.deck.bolts.clear();
        g.deck.pilot = (20, 40);
        g.deck.facing = (0, 1);
        g
    }

    #[test]
    fn the_sabre_cuts_what_is_in_front_of_it_and_the_blaster_does_not_reach() {
        let mut g = planetside();
        assert_eq!(
            g.deck.sidearm,
            SideArm::ServicePistol,
            "the issued sidearm is on the belt"
        );
        g.credits = 20_000;
        g.status = Status::Hangar;
        assert!(g.buy_sidearm(SideArm::Sabre), "a blade is bought");
        g.status = Status::Surface;
        assert_eq!(g.deck.sidearm, SideArm::Sabre, "and it lights in your hand");
        g.deck.blaster_cooldown = 0;
        g.deck.troopers = vec![
            Trooper::new(GroundKind::Trooper, (20, 41), 99),
            Trooper::new(GroundKind::Trooper, (20, 48), 99),
        ];
        assert!(g.shoot(), "the cut goes in");
        assert_eq!(g.deck.troopers.len(), 1, "the near one is cut down");
        assert_eq!(
            g.deck.troopers[0].pos,
            (20, 48),
            "and the far one is well out of reach"
        );
        assert!(g.deck.bolts.is_empty(), "a sabre puts nothing in the air");
    }

    #[test]
    fn a_sabre_turns_a_bolt_back_the_way_it_came() {
        let mut g = planetside();
        g.deck.sidearm = SideArm::Sabre;
        g.deck.bolts = vec![Bolt {
            pos: (20, 41),
            dir: (0, -1),
            friendly: false,
            life: 8,
        }];
        g.deck.skirmish(1);
        assert!(!g.deck.bolts.is_empty(), "the bolt is still in the air");
        assert!(
            g.deck.bolts.iter().all(|b| b.friendly && b.dir == (0, 1)),
            "and it is going back the other way, on our side"
        );
        assert_eq!(g.deck.health, PILOT_HEALTH, "nothing got through");
    }

    #[test]
    fn a_force_shove_puts_a_patrol_on_its_back() {
        let mut g = planetside();
        g.force = FORCE_MAX;
        g.deck.troopers = vec![Trooper::new(GroundKind::Trooper, (20, 42), 0)];
        assert!(g.force_push(), "the Force answers");
        assert!(g.force < FORCE_MAX, "and it costs");
        match g.deck.troopers.first() {
            Some(trooper) => assert!(trooper.pos.1 > 42, "he is thrown back"),
            None => {} // Thrown hard enough to finish him, which is fine.
        }
        g.force = 0;
        assert!(!g.force_push(), "an empty pilot shoves nothing");
    }

    #[test]
    fn a_scout_walker_is_more_than_a_sidearm_can_handle_in_one_go() {
        let mut g = planetside();
        g.deck.troopers = vec![Trooper::new(GroundKind::Scout, (20, 42), 0)];
        assert_eq!(g.deck.troopers[0].hp, GroundKind::Scout.hp());
        assert!(
            GroundKind::Scout.hp() > GroundKind::Trooper.hp() * 4,
            "it is armoured"
        );
        g.deck.sidearm = SideArm::Sabre;
        g.shoot();
        assert!(!g.deck.troopers.is_empty(), "one cut does not fell it");
        g.deck.troopers[0].cooldown = 0;
        g.deck.skirmish(1);
        assert!(
            g.deck.bolts.iter().filter(|b| !b.friendly).count() >= 2,
            "and its chin gun fires in pairs"
        );
    }

    #[test]
    fn a_duellist_closes_and_cuts_rather_than_shooting() {
        let mut g = planetside();
        g.deck.troopers = vec![Trooper::new(GroundKind::Duellist, (20, 41), 0)];
        // Give him the length of a wind-up to get a cut in.
        let mut hurt = 0;
        for tick in 0..40 {
            hurt += g.deck.skirmish(tick);
        }
        assert!(hurt > 0, "he is inside your guard and cutting");
        assert!(
            g.deck.bolts.iter().all(|b| b.friendly),
            "and he does not shoot"
        );
        assert!(
            GroundKind::Duellist.reach() < GroundKind::Trooper.reach(),
            "he has to be close to do it"
        );
    }

    #[test]
    fn a_speeder_covers_ground_fast_and_a_tree_ends_the_ride() {
        let mut g = planetside();
        g.deck.pilot = (20, 20);
        g.deck.facing = (0, 1);
        assert!(g.mount(), "on the speeder");
        g.step_ahead(1);
        assert_eq!(g.deck.pilot.1, 20 + SPEEDER_PACE, "three cells a step");
        g.deck.cover = vec![Cover {
            pos: (20, g.deck.pilot.1 + 1),
            size: (1, 1),
            tall: false,
        }];
        let health = g.deck.health;
        g.step_ahead(1);
        assert!(!g.deck.riding, "the ride ends in the tree");
        assert_eq!(g.deck.health, health - SPEEDER_CRASH, "and it hurts");
    }

    #[test]
    fn a_bounty_hunter_takes_the_contract_and_closes_on_you() {
        let mut g = flying();
        g.status = Status::Chart;
        g.score = HUNTER_BOUNTY + 1;
        g.map.cursor = g.map.reachable()[0];
        assert!(g.jump(), "the lane is flown");
        assert!(g.hunter.is_some(), "somebody posts the bounty");
        let start = g.hunter.unwrap();
        assert_ne!(start, g.map.at, "he starts a long way off");
        // Fly on and he keeps coming.
        for _ in 0..12 {
            if g.status == Status::Hangar {
                g.open_chart();
            }
            if g.status == Status::Chart {
                g.map.cursor = g.map.reachable()[0];
                g.jump();
            }
            g.status = Status::Chart;
        }
        let hunter = g.hunter.expect("still on the trail");
        let (hx, hy) = g.map.nodes[hunter].pos;
        let (sx, sy) = g.map.nodes[g.map.at].pos;
        let (ox, oy) = g.map.nodes[start].pos;
        assert!(
            hx.abs_diff(sx) + hy.abs_diff(sy) < ox.abs_diff(sx) + oy.abs_diff(sy) + 12,
            "and he is closer than where he started"
        );
    }

    #[test]
    fn catching_the_hunter_in_a_system_is_a_fight_and_killing_him_pays() {
        let mut g = flying();
        g.hunter = Some(g.map.at);
        assert!(g.hunted_here(), "he is sitting in this system");
        g.spawn_wave();
        let boss = g.boss.as_ref().expect("he is waiting");
        assert_eq!(boss.kind, BossKind::AceTie, "and he flies something quick");
        assert!(
            g.enemies.iter().any(|e| e.kind == EnemyKind::TieAdvanced),
            "with friends"
        );
        g.boss.as_mut().unwrap().hp = 0;
        let credits = g.credits;
        g.check_end();
        assert!(g.hunter.is_none(), "that is the hunter dealt with");
        assert!(g.credits > credits, "and his ship is worth something");
    }
}

#[cfg(test)]
mod duel_tests {
    use super::tests::flying;
    use super::*;

    fn crossed_blades() -> Game {
        let mut g = flying();
        g.status = Status::Surface;
        g.planet = Planet::Dagobah;
        g.deck = Deck::surface(Planet::Dagobah, 41);
        g.deck.cover.clear();
        g.deck.bolts.clear();
        g.deck.pilot = (20, 40);
        g.deck.facing = (0, 1);
        g.armoury = vec![SideArm::ServicePistol, SideArm::Sabre];
        g.deck.hold(SideArm::Sabre);
        g.deck.blaster_cooldown = 0;
        g.deck.troopers = vec![Trooper::new(GroundKind::Duellist, (20, 42), 99)];
        g.deck.skirmish(1);
        g
    }

    #[test]
    fn crossing_blades_starts_a_duel_and_walking_off_ends_it() {
        let mut g = crossed_blades();
        let duel = g.deck.duel.as_ref().expect("blades are crossed");
        assert_eq!(duel.foe, 0);
        assert_eq!(duel.poise, DUEL_POISE, "both start on their feet");
        g.deck.pilot = (20, 4);
        g.deck.skirmish(2);
        assert!(g.deck.duel.is_none(), "out of reach, out of the duel");
    }

    #[test]
    fn he_winds_up_before_he_cuts_and_a_guard_turns_it() {
        let mut g = crossed_blades();
        // Let him start a swing, then get the blade up in time.
        let mut ticks = 0;
        while g.deck.duel.as_ref().is_some_and(|d| d.wind_up == 0) && ticks < 30 {
            g.deck.skirmish(ticks);
            ticks += 1;
        }
        assert!(
            g.deck.duel.as_ref().is_some_and(|d| d.wind_up > 0),
            "the wind-up is visible"
        );
        let health = g.deck.health;
        while g.deck.duel.as_ref().is_some_and(|d| d.wind_up > 1) {
            g.deck.skirmish(ticks);
            ticks += 1;
        }
        assert!(g.parry(), "the guard goes up");
        g.deck.skirmish(ticks);
        assert_eq!(g.deck.health, health, "and nothing lands");
        let duel = g.deck.duel.as_ref().unwrap();
        assert!(duel.riposte > 0, "which opens him up for a moment");
        assert!(duel.foe_poise < DUEL_POISE, "and costs him his footing");
    }

    #[test]
    fn a_cut_that_is_not_parried_lands_and_costs_footing() {
        let mut g = crossed_blades();
        let health = g.deck.health;
        for tick in 0..40 {
            g.ground_tick();
            let _ = tick;
            if g.deck.health < health {
                break;
            }
        }
        assert!(g.deck.health < health, "standing there gets you cut");
    }

    #[test]
    fn a_riposte_cuts_twice_as_deep() {
        let mut g = crossed_blades();
        {
            let duel = g.deck.duel.as_mut().unwrap();
            duel.riposte = RIPOSTE_TICKS;
        }
        let hp = g.deck.troopers[0].hp;
        g.deck.blaster_cooldown = 0;
        g.shoot();
        let after = g.deck.troopers.first().map_or(0, |t| t.hp);
        assert!(
            hp - after >= SideArm::Sabre.damage() * 2,
            "a riposte lands twice as hard: {hp} to {after}"
        );
    }

    #[test]
    fn two_blades_arriving_together_bind_and_the_lock_is_won_by_leaning_on_it() {
        let mut g = crossed_blades();
        {
            let duel = g.deck.duel.as_mut().unwrap();
            duel.wind_up = PARRY_WINDOW;
        }
        g.deck.blaster_cooldown = 0;
        g.shoot();
        assert!(
            g.deck.duel.as_ref().is_some_and(|d| d.lock > 0),
            "the blades bind"
        );
        for _ in 0..3 {
            g.deck.blaster_cooldown = 0;
            g.shoot();
        }
        while g.deck.duel.as_ref().is_some_and(|d| d.lock > 0) {
            g.deck.skirmish(1);
        }
        let duel = g.deck.duel.as_ref().unwrap();
        assert!(duel.foe_stagger > 0, "leaning on it puts him off balance");
        assert_eq!(duel.stagger, 0, "and you keep your feet");
    }

    #[test]
    fn a_stance_changes_how_long_he_takes_and_how_hard_he_hits() {
        assert!(
            Stance::Aggressive.wind_up() < Stance::Defensive.wind_up(),
            "pressing is quicker than waiting"
        );
        assert!(
            Stance::Aggressive.damage() > Stance::Measured.damage(),
            "and it costs more when it lands"
        );
        assert_eq!(Stance::Defensive.name(), "guarded");
    }
}

#[cfg(test)]
mod armoury_tests {
    use super::tests::flying;
    use super::*;

    fn at_the_rack() -> Game {
        let mut g = flying();
        g.status = Status::Hangar;
        g.credits = 30_000;
        g
    }

    #[test]
    fn the_rack_carries_every_blaster_and_every_blade() {
        assert_eq!(SideArm::ALL.len(), 13, "nine barrels and four blades");
        let blades = SideArm::ALL.iter().filter(|arm| arm.is_blade()).count();
        assert_eq!(blades, 4, "sabre, duals, staff and crossguard");
        assert!(
            SideArm::Disruptor.damage() > SideArm::HoldOut.damage(),
            "a disruptor is not a hold-out"
        );
        assert!(
            SideArm::Repeater.cadence() < SideArm::LongRifle.cadence(),
            "a repeater hoses and a rifle does not"
        );
        assert!(
            SideArm::LongRifle.range() > SideArm::Scattergun.range(),
            "and reach is what you pay a rifle for"
        );
        assert_eq!(
            SideArm::Scattergun.pellets(),
            3,
            "a scattergun throws three"
        );
    }

    #[test]
    fn buying_puts_it_on_the_belt_and_selling_gives_half_back() {
        let mut g = at_the_rack();
        assert_eq!(
            g.armoury,
            vec![SideArm::ServicePistol],
            "one issued sidearm"
        );
        let credits = g.credits;
        assert!(g.buy_sidearm(SideArm::Bowcaster), "the bowcaster is bought");
        assert_eq!(g.credits, credits - SideArm::Bowcaster.price());
        assert_eq!(
            g.deck.sidearm,
            SideArm::Bowcaster,
            "and it is in your hands"
        );
        assert!(!g.buy_sidearm(SideArm::Bowcaster), "you only need the one");
        assert!(
            !g.sell_sidearm(SideArm::Bowcaster),
            "and you cannot sell what you are holding"
        );
        g.deck.hold(SideArm::ServicePistol);
        let before = g.credits;
        assert!(g.sell_sidearm(SideArm::Bowcaster), "off the belt it goes");
        assert_eq!(g.credits, before + SideArm::Bowcaster.resale());
        assert!(
            !g.sell_sidearm(SideArm::ServicePistol),
            "nobody walks a world unarmed"
        );
    }

    #[test]
    fn the_rack_keys_buy_and_then_sell_the_same_line() {
        let mut g = at_the_rack();
        let (key, arm, _, owned, can) = g
            .armoury_lines()
            .into_iter()
            .find(|line| line.1 == SideArm::Carbine)
            .expect("the carbine is on the rack");
        assert!(!owned && can, "not owned, and affordable");
        assert!(g.trade_sidearm(key), "the key buys it");
        assert!(g.armoury.contains(&SideArm::Carbine));
        g.deck.hold(SideArm::ServicePistol);
        assert!(g.trade_sidearm(key), "and the same key sells it back");
        assert!(!g.armoury.contains(&SideArm::Carbine));
        let _ = arm;
    }

    #[test]
    fn what_is_in_your_hands_is_what_shoots() {
        let mut g = flying();
        g.status = Status::Surface;
        g.deck = Deck::surface(Planet::Tatooine, 7);
        g.deck.cover.clear();
        g.deck.troopers.clear();
        g.deck.bolts.clear();
        g.deck.pilot = (20, 20);
        g.deck.facing = (0, 1);
        g.deck.hold(SideArm::Scattergun);
        g.deck.blaster_cooldown = 0;
        assert!(g.shoot(), "the scattergun fires");
        assert_eq!(g.deck.bolts.len(), 3, "three pellets");
        g.deck.bolts.clear();
        g.deck.hold(SideArm::LongRifle);
        g.deck.blaster_cooldown = 0;
        g.shoot();
        assert_eq!(g.deck.bolts.len(), 1, "a rifle sends one");
        assert_eq!(
            g.deck.bolts[0].life,
            SideArm::LongRifle.range(),
            "and it carries further"
        );
    }

    #[test]
    fn the_belt_cycles_through_what_you_own() {
        let mut g = at_the_rack();
        g.buy_sidearm(SideArm::Sabre);
        g.buy_sidearm(SideArm::HeavyPistol);
        let first = g.deck.sidearm;
        let second = g.draw_sidearm().expect("something else on the belt");
        assert_ne!(second, first, "the next thing comes to hand");
        let mut seen = vec![first, second];
        for _ in 0..3 {
            seen.push(g.draw_sidearm().unwrap());
        }
        assert!(
            seen.contains(&SideArm::Sabre) && seen.contains(&SideArm::HeavyPistol),
            "and everything on the belt comes round"
        );
    }

    #[test]
    fn a_blade_reaches_further_round_than_a_single_one() {
        assert!(
            SideArm::SabreStaff.arc() > SideArm::Sabre.arc(),
            "a staff comes round further"
        );
        assert!(
            SideArm::CrossguardSabre.damage() > SideArm::DualSabres.damage(),
            "and a crossguard hits harder than a pair"
        );
        assert!(
            SideArm::DualSabres.cadence() < SideArm::CrossguardSabre.cadence(),
            "though the pair are quicker"
        );
    }
}
