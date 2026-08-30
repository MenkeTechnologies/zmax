//! Nova — the heavy-weapons formation shooter.
//!
//! Where `galaga` is the plain arcade original, Nova is the modern shoot-'em-up
//! built on the same court: pick one of three hulls, fly anywhere in the lower
//! third of the field, and swap between five guns dropped by the enemies you
//! kill. The formation mixes six enemy types — grunts that peel off and dive,
//! weavers that snake down the court, aimed turrets, spread-firing bombers,
//! kamikazes and armoured tanks — and every fourth wave is a boss with a health
//! bar and three attack phases. Shields soak hits before a life is lost, smart
//! bombs clear the screen, and a kill chain multiplies the score while it lasts.
//!
//! Controls: `←/→`/`h`/`l` and `↑/↓`/`k`/`j` fly, `SPC` (or `f`) fires, `b`
//! drops a smart bomb, `p` pauses, `n` restarts, `q`/`Esc` quits. The ship
//! picker takes `1`/`2`/`3` or `←/→` plus `Enter`.
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
const W: i16 = 54;
/// Court height in cells.
const H: i16 = 22;
/// Topmost row the ship may fly to; it owns the bottom seven rows.
const SHIP_TOP: i16 = H - 7;
/// The row the ship starts on.
const SHIP_ROW: i16 = H - 1;
/// Formation columns.
const COLS: usize = 9;
/// Formation rows.
const ROWS: usize = 4;
/// Horizontal spacing between formation columns.
const ENEMY_GAP: i16 = 5;
/// Column of the leftmost formation column at zero sway.
const BASE_X: i16 = 4;
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
/// Cap on player shots in flight.
const MAX_SHOTS: usize = 24;
/// Damage a smart bomb deals to everything on the court.
const BOMB_DAMAGE: i32 = 4;
/// Half-width of the boss hull; it spans `2 * BOSS_HALF + 1` columns.
const BOSS_HALF: i16 = 4;
/// Ticks between a cleared wave and the next one.
const INTERMISSION_TICKS: u32 = 24;
/// 1-in-N chance a kill drops a powerup.
const DROP_CHANCE: u64 = 4;
/// Every Nth wave is a boss wave.
const BOSS_EVERY: u32 = 4;

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

    /// Columns the hull slides per keypress.
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

    /// Ticks between shots.
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

    pub fn blurb(self) -> &'static str {
        match self {
            ShipClass::Interceptor => "fast, fragile, fires twice as often",
            ShipClass::Cruiser => "the balanced hull",
            ShipClass::Juggernaut => "slow and heavy, four shield pips",
        }
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
}

impl Weapon {
    pub const ALL: [Weapon; 5] = [
        Weapon::Blaster,
        Weapon::Spread,
        Weapon::Laser,
        Weapon::Homing,
        Weapon::Plasma,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Weapon::Blaster => "blaster",
            Weapon::Spread => "spread",
            Weapon::Laser => "laser",
            Weapon::Homing => "homing",
            Weapon::Plasma => "plasma",
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
        }
    }
}

/// What a shot looks like, and by extension how it reads on the court.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShotKind {
    Bolt,
    Beam,
    Missile,
    Plasma,
    Enemy,
}

impl ShotKind {
    pub fn glyph(self) -> &'static str {
        match self {
            ShotKind::Bolt => "|",
            ShotKind::Beam => "┃",
            ShotKind::Missile => "↟",
            ShotKind::Plasma => "◍",
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
            kind: ShotKind::Bolt,
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
}

/// The six hulls that fly against you.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnemyKind {
    Grunt,
    Weaver,
    Turret,
    Bomber,
    Kamikaze,
    Tank,
}

impl EnemyKind {
    pub fn hp(self) -> i32 {
        match self {
            EnemyKind::Grunt | EnemyKind::Kamikaze => 1,
            EnemyKind::Weaver => 2,
            EnemyKind::Turret => 3,
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
            EnemyKind::Bomber => 40,
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
        }
    }

    /// 1-in-N chance per tick of peeling out of formation; `0` never dives.
    fn dive_chance(self) -> u64 {
        match self {
            EnemyKind::Grunt => 140,
            EnemyKind::Kamikaze => 70,
            EnemyKind::Bomber => 220,
            _ => 0,
        }
    }

    /// 1-in-N chance per tick of shooting; `0` never shoots.
    fn fire_chance(self) -> u64 {
        match self {
            EnemyKind::Grunt => 180,
            EnemyKind::Turret => 60,
            EnemyKind::Bomber => 90,
            EnemyKind::Tank => 120,
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
    pub state: EnemyState,
}

impl Enemy {
    pub fn new(kind: EnemyKind, home: (i16, i16)) -> Enemy {
        Enemy {
            kind,
            pos: home,
            home,
            hp: kind.hp(),
            state: EnemyState::Formation,
        }
    }
}

/// What a dropped pickup gives you.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerKind {
    /// A gun: the one you carry levels up, any other replaces it.
    Gun(Weapon),
    Shield,
    Bomb,
    Rapid,
    Life,
}

impl PowerKind {
    pub fn glyph(self) -> &'static str {
        match self {
            PowerKind::Gun(w) => w.tag(),
            PowerKind::Shield => "◈",
            PowerKind::Bomb => "◆",
            PowerKind::Rapid => "»",
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

/// The wave boss: a wide hull that sweeps the top of the court and escalates
/// through three attack phases as its armour burns off.
#[derive(Clone, Debug)]
pub struct Boss {
    pub pos: (i16, i16),
    pub hp: i32,
    pub max_hp: i32,
    pub dir: i16,
    cooldown: u32,
    minion_timer: u32,
}

impl Boss {
    /// `1` above two thirds health, `2` down to a third, `3` once enraged.
    pub fn phase(&self) -> u8 {
        match self.hp.max(0) * 3 / self.max_hp.max(1) {
            f if f >= 2 => 1,
            1 => 2,
            _ => 3,
        }
    }

    /// Columns it sweeps per tick.
    fn speed(&self) -> i16 {
        if self.phase() == 3 {
            2
        } else {
            1
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

/// Where a round is: picking a hull, flying, between waves, or over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Select,
    Playing,
    WaveClear,
    Lost,
}

/// The pure Nova court. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    pub class: ShipClass,
    /// Ship position as `(row, col)`; it flies the bottom seven rows.
    pub ship: (i16, i16),
    pub weapon: Weapon,
    pub weapon_level: u32,
    pub shield: u32,
    pub max_shield: u32,
    pub lives: u32,
    pub bombs: u32,
    pub score: u32,
    /// Kill-chain multiplier, `1` when cold.
    pub combo: u32,
    pub wave: u32,
    pub status: Status,
    pub enemies: Vec<Enemy>,
    pub boss: Option<Boss>,
    pub shots: Vec<Shot>,
    pub enemy_shots: Vec<Shot>,
    pub powerups: Vec<Powerup>,
    /// Frames of smart-bomb flash left, for the renderer.
    pub flash: u32,
    /// Ticks left of the between-waves pause.
    pub intermission: u32,
    sway_x: i16,
    sway_dir: i16,
    sway_counter: u32,
    fire_cooldown: u32,
    invuln: u32,
    combo_timer: u32,
    rapid: u32,
    tick: u32,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        Game {
            class: ShipClass::Cruiser,
            ship: (SHIP_ROW, W / 2),
            weapon: Weapon::Blaster,
            weapon_level: 1,
            shield: 0,
            max_shield: 0,
            lives: 3,
            bombs: 0,
            score: 0,
            combo: 1,
            wave: 1,
            status: Status::Select,
            enemies: Vec::new(),
            boss: None,
            shots: Vec::new(),
            enemy_shots: Vec::new(),
            powerups: Vec::new(),
            flash: 0,
            intermission: 0,
            sway_x: 0,
            sway_dir: 1,
            sway_counter: SWAY_CADENCE,
            fire_cooldown: 0,
            invuln: 0,
            combo_timer: 0,
            rapid: 0,
            tick: 0,
            rng: seed | 1,
        }
    }

    /// Commit to a hull and fly wave one.
    pub fn start(&mut self, class: ShipClass) {
        self.class = class;
        self.ship = (SHIP_ROW, W / 2);
        self.weapon = Weapon::Blaster;
        self.weapon_level = 1;
        self.max_shield = class.max_shield();
        self.shield = self.max_shield;
        self.bombs = class.bombs();
        self.lives = 3;
        self.score = 0;
        self.combo = 1;
        self.wave = 1;
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

    /// True while the ship is still flashing from its last hit.
    pub fn invulnerable(&self) -> bool {
        self.invuln > 0
    }

    /// True while a rapid-fire pickup is running.
    pub fn rapid_active(&self) -> bool {
        self.rapid > 0
    }

    /// Which hull sits in a formation slot: later waves seed tougher ones, and
    /// the front row is the one that peels off and dives.
    fn wave_kind(&self, row: usize, col: usize) -> EnemyKind {
        let w = self.wave;
        match row {
            0 if w >= 2 && col % 3 == 1 => EnemyKind::Turret,
            0 => EnemyKind::Grunt,
            1 if w >= 3 && col % 4 == 2 => EnemyKind::Bomber,
            1 => EnemyKind::Weaver,
            2 if w >= 5 && col % 5 == 0 => EnemyKind::Tank,
            2 => EnemyKind::Grunt,
            _ => EnemyKind::Kamikaze,
        }
    }

    /// Build the current wave: a boss with a kamikaze escort every
    /// `BOSS_EVERY`th wave, otherwise a mixed formation.
    pub fn spawn_wave(&mut self) {
        self.enemies.clear();
        self.boss = None;
        self.shots.clear();
        self.enemy_shots.clear();
        self.powerups.clear();
        self.sway_x = 0;
        self.sway_dir = 1;
        self.sway_counter = SWAY_CADENCE;
        if self.wave.is_multiple_of(BOSS_EVERY) {
            let hp = 60 + 40 * (self.wave / BOSS_EVERY) as i32;
            self.boss = Some(Boss {
                pos: (FORMATION_TOP, W / 2),
                hp,
                max_hp: hp,
                dir: 1,
                cooldown: 20,
                minion_timer: 90,
            });
            for col in (0..COLS).step_by(2) {
                let home = (FORMATION_TOP + 4, BASE_X + col as i16 * ENEMY_GAP);
                self.enemies.push(Enemy::new(EnemyKind::Kamikaze, home));
            }
            return;
        }
        let rows = (2 + self.wave.min(2) as usize).min(ROWS);
        for row in 0..rows {
            for col in 0..COLS {
                let kind = self.wave_kind(row, col);
                let home = (
                    FORMATION_TOP + row as i16 * 2,
                    BASE_X + col as i16 * ENEMY_GAP,
                );
                self.enemies.push(Enemy::new(kind, home));
            }
        }
    }

    /// Roll into the next wave, keeping score, guns and lives; a cleared wave
    /// tops the shields back up and pays out a spare bomb.
    fn next_wave(&mut self) {
        self.wave += 1;
        self.shield = self.max_shield;
        self.bombs += 1;
        self.status = Status::Playing;
        self.spawn_wave();
    }

    /// Fly the ship, clamped to the court and to its own bottom-third box.
    pub fn move_ship(&mut self, dc: i16, dr: i16) {
        if self.status != Status::Playing {
            return;
        }
        self.ship.1 = (self.ship.1 + dc * self.class.speed()).clamp(1, W - 2);
        self.ship.0 = (self.ship.0 + dr).clamp(SHIP_TOP, SHIP_ROW);
    }

    /// Fire the current gun if its cadence has come round again.
    pub fn fire(&mut self) {
        if self.status != Status::Playing || self.fire_cooldown > 0 || self.shots.len() >= MAX_SHOTS
        {
            return;
        }
        let cadence = self.class.fire_cadence();
        self.fire_cooldown = if self.rapid > 0 {
            cadence.div_ceil(2)
        } else {
            cadence
        };
        let level = self.weapon_level;
        let dmg = self.class.damage() + level as i32 - 1;
        let (r, c) = (self.ship.0 - 1, self.ship.1);
        match self.weapon {
            Weapon::Blaster => {
                let lanes: &[i16] = match level {
                    1 => &[0],
                    2 => &[-1, 1],
                    _ => &[-1, 0, 1],
                };
                for &dx in lanes {
                    self.shots.push(Shot::bolt((r, c + dx), 0, dmg + 1));
                }
            }
            Weapon::Spread => {
                let lanes: &[i16] = if level >= 2 {
                    &[-2, -1, 0, 1, 2]
                } else {
                    &[-1, 0, 1]
                };
                for &drift in lanes {
                    self.shots.push(Shot::bolt((r, c), drift, dmg));
                }
            }
            Weapon::Laser => {
                let lanes: &[i16] = match level {
                    1 => &[0],
                    2 => &[-1, 1],
                    _ => &[-1, 0, 1],
                };
                for &dx in lanes {
                    self.shots.push(Shot::beam((r, c + dx), dmg));
                }
            }
            Weapon::Homing => {
                let lanes: &[i16] = match level {
                    1 => &[0],
                    2 => &[-2, 2],
                    _ => &[-2, 0, 2],
                };
                for &dx in lanes {
                    self.shots.push(Shot::missile((r, c + dx), dmg + 1));
                }
            }
            Weapon::Plasma => {
                let half_width = if level >= 3 { 2 } else { 1 };
                self.shots.push(Shot::plasma((r, c), dmg + 2, half_width));
            }
        }
    }

    /// Drop a smart bomb: every enemy shot is wiped and everything on the court
    /// takes damage, the boss included.
    pub fn bomb(&mut self) {
        if self.status != Status::Playing || self.bombs == 0 {
            return;
        }
        self.bombs -= 1;
        self.flash = 6;
        self.enemy_shots.clear();
        let mut survivors = Vec::with_capacity(self.enemies.len());
        for mut e in std::mem::take(&mut self.enemies) {
            e.hp -= BOMB_DAMAGE;
            if e.hp <= 0 {
                self.score += e.kind.score();
            } else {
                survivors.push(e);
            }
        }
        self.enemies = survivors;
        if let Some(boss) = self.boss.as_mut() {
            boss.hp -= (boss.max_hp / 12).max(BOMB_DAMAGE);
        }
        self.check_end();
    }

    /// Take a hit: shields soak first, then a life goes and the gun drops a
    /// level. Either way the chain breaks and the hull flashes for a moment.
    fn damage_ship(&mut self) {
        if self.invuln > 0 {
            return;
        }
        self.invuln = INVULN_TICKS;
        self.combo = 1;
        self.combo_timer = 0;
        if self.shield > 0 {
            self.shield -= 1;
            return;
        }
        self.lives = self.lives.saturating_sub(1);
        self.shield = self.max_shield;
        self.weapon_level = self.weapon_level.saturating_sub(1).max(1);
    }

    /// Bank a kill at the current chain multiplier and extend the chain.
    fn award(&mut self, base: u32) {
        self.score += base * self.combo;
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
            PowerKind::Life => self.lives += 1,
        }
        self.score += 25;
    }

    /// What a kill drops: guns half the time, then armour, rapid fire, a bomb
    /// and — rarely — a spare life.
    fn roll_power(&mut self) -> PowerKind {
        match self.rand() % 12 {
            0..=5 => {
                let i = (self.rand() % Weapon::ALL.len() as u64) as usize;
                PowerKind::Gun(Weapon::ALL[i])
            }
            6 | 7 => PowerKind::Shield,
            8 | 9 => PowerKind::Rapid,
            10 => PowerKind::Bomb,
            _ => PowerKind::Life,
        }
    }

    /// Age every timer by one tick, cooling the kill chain when it lapses.
    fn tick_timers(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.fire_cooldown = self.fire_cooldown.saturating_sub(1);
        self.invuln = self.invuln.saturating_sub(1);
        self.rapid = self.rapid.saturating_sub(1);
        self.flash = self.flash.saturating_sub(1);
        if self.combo_timer > 0 {
            self.combo_timer -= 1;
            if self.combo_timer == 0 {
                self.combo = 1;
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

    /// Advance every enemy: hold formation, dive, or weave; shoot on the way;
    /// ram the ship if it is in the way. A diver that runs off the bottom comes
    /// back round to its slot, but a kamikaze that misses is gone for good.
    fn advance_enemies(&mut self) {
        let ship = self.ship;
        let sway = self.sway_x;
        let tick = self.tick;
        let mut spawned: Vec<Shot> = Vec::new();
        let mut kept = Vec::with_capacity(self.enemies.len());
        let mut rammed = false;
        for mut e in std::mem::take(&mut self.enemies) {
            match e.state {
                EnemyState::Formation => {
                    e.pos = (e.home.0, e.home.1 + sway);
                    let dive = e.kind.dive_chance();
                    if dive > 0 && self.rand().is_multiple_of(dive) {
                        e.state = EnemyState::Diving { target_x: ship.1 };
                    } else if e.kind == EnemyKind::Weaver
                        && self.rand().is_multiple_of(WEAVE_CHANCE)
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
            let fire = e.kind.fire_chance();
            if fire > 0 && self.rand().is_multiple_of(fire) {
                let muzzle = (e.pos.0 + 1, e.pos.1);
                match e.kind {
                    // Turrets lead the ship, bombers throw a three-way spread.
                    EnemyKind::Turret => {
                        spawned.push(Shot::enemy(muzzle, (ship.1 - e.pos.1).signum(), 1));
                    }
                    EnemyKind::Bomber => {
                        for drift in [-1, 0, 1] {
                            spawned.push(Shot::enemy(muzzle, drift, 1));
                        }
                    }
                    _ => spawned.push(Shot::enemy(muzzle, 0, 1)),
                }
            }
            kept.push(e);
        }
        self.enemies = kept;
        self.enemy_shots.extend(spawned);
        if rammed {
            self.damage_ship();
        }
    }

    /// Sweep the boss across the top of the court and run its phase pattern:
    /// aimed volleys, then a widening fan plus kamikaze escorts, then enraged
    /// lane fire at double speed.
    fn advance_boss(&mut self) {
        let Some(mut boss) = self.boss.take() else {
            return;
        };
        let ship = self.ship;
        let next = boss.pos.1 + boss.dir * boss.speed();
        if (BOSS_HALF + 1..W - BOSS_HALF - 1).contains(&next) {
            boss.pos.1 = next;
        } else {
            boss.dir = -boss.dir;
        }
        if boss.cooldown > 0 {
            boss.cooldown -= 1;
        } else {
            boss.cooldown = boss.cadence();
            let row = boss.pos.0 + 2;
            let aim = (ship.1 - boss.pos.1).signum();
            match boss.phase() {
                1 => {
                    for dx in [-BOSS_HALF, 0, BOSS_HALF] {
                        self.enemy_shots
                            .push(Shot::enemy((row, boss.pos.1 + dx), aim, 1));
                    }
                }
                2 => {
                    for drift in -2..=2 {
                        self.enemy_shots
                            .push(Shot::enemy((row, boss.pos.1), drift, 1));
                    }
                }
                _ => {
                    for lane in [W / 6, W / 2, 5 * W / 6] {
                        self.enemy_shots.push(Shot::enemy((row, lane), 0, 2));
                    }
                    self.enemy_shots
                        .push(Shot::enemy((row, boss.pos.1), aim, 2));
                }
            }
        }
        if boss.phase() >= 2 {
            if boss.minion_timer > 0 {
                boss.minion_timer -= 1;
            } else {
                boss.minion_timer = 80;
                for dx in [-BOSS_HALF, BOSS_HALF] {
                    let home = (boss.pos.0 + 3, (boss.pos.1 + dx).clamp(1, W - 2));
                    let mut e = Enemy::new(EnemyKind::Kamikaze, home);
                    e.state = EnemyState::Diving { target_x: ship.1 };
                    self.enemies.push(e);
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

    /// Damage everything under a shot's footprint; returns whether it connected.
    fn hit_targets(&mut self, shot: &Shot) -> bool {
        let (r, c) = shot.pos;
        let mut hit = false;
        if let Some(boss) = self.boss.as_mut() {
            let rows = boss.pos.0..=boss.pos.0 + 1;
            let cols = boss.pos.1 - BOSS_HALF..=boss.pos.1 + BOSS_HALF;
            if rows.contains(&r) && cols.contains(&c) {
                boss.hp -= shot.damage;
                hit = true;
            }
        }
        let mut kills: Vec<(EnemyKind, (i16, i16))> = Vec::new();
        let mut kept = Vec::with_capacity(self.enemies.len());
        for mut e in std::mem::take(&mut self.enemies) {
            if e.pos.0 == r && (e.pos.1 - c).abs() <= shot.half_width {
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
        for (kind, pos) in kills {
            self.award(kind.score());
            if self.rand().is_multiple_of(DROP_CHANCE) {
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
                if self.hit_targets(&s) && !s.pierce {
                    continue 'shot;
                }
            }
            kept.push(s);
        }
        self.shots = kept;
    }

    /// Advance enemy fire; anything reaching the ship's cell is a hit.
    fn advance_enemy_shots(&mut self) {
        let ship = self.ship;
        let mut kept = Vec::with_capacity(self.enemy_shots.len());
        let mut hits = 0;
        'shot: for mut s in std::mem::take(&mut self.enemy_shots) {
            for step in 0..s.speed.unsigned_abs() as i16 {
                s.pos.0 += 1;
                if step == 0 {
                    s.pos.1 += s.drift;
                }
                if s.pos.0 >= H || !(0..W).contains(&s.pos.1) {
                    continue 'shot;
                }
                if s.pos == ship {
                    hits += 1;
                    continue 'shot;
                }
            }
            kept.push(s);
        }
        self.enemy_shots = kept;
        for _ in 0..hits {
            self.damage_ship();
        }
    }

    /// Tumble pickups down the court at half speed and collect the ones the
    /// ship flies into.
    fn advance_powerups(&mut self) {
        let ship = self.ship;
        let falling = self.tick.is_multiple_of(2);
        let mut kept = Vec::with_capacity(self.powerups.len());
        let mut taken = Vec::new();
        for mut p in std::mem::take(&mut self.powerups) {
            if falling {
                p.pos.0 += 1;
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

    /// Settle the round: a dead boss pays a bounty, no lives ends it, and an
    /// empty court starts the between-waves pause.
    fn check_end(&mut self) {
        if self.boss.as_ref().is_some_and(|b| b.hp <= 0) {
            self.boss = None;
            self.score += 500 * self.wave;
        }
        if self.lives == 0 {
            self.status = Status::Lost;
            return;
        }
        if self.enemies.is_empty() && self.boss.is_none() && self.status == Status::Playing {
            self.status = Status::WaveClear;
            self.intermission = INTERMISSION_TICKS;
            self.score += 100 * self.wave;
        }
    }

    /// Advance one tick of the round, or of the pause between waves.
    pub fn step(&mut self) {
        match self.status {
            Status::Playing => {
                self.tick_timers();
                self.sway();
                self.advance_enemies();
                self.advance_boss();
                self.advance_shots();
                self.advance_enemy_shots();
                self.advance_powerups();
                self.check_end();
            }
            Status::WaveClear => {
                self.intermission = self.intermission.saturating_sub(1);
                if self.intermission == 0 {
                    self.next_wave();
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

    /// Straight back into a round with the same hull.
    fn retry(&mut self) {
        let class = self.game.class;
        self.restart();
        self.game.start(class);
    }

    /// Running = a live round or the pause between waves, and not paused.
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
            "Pick a hull — ←/→ or 1/2/3, Enter to launch, q to quit.",
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
                    "shield {}  dmg {}  cadence {}  bombs {}",
                    class.max_shield(),
                    class.damage(),
                    class.fire_cadence(),
                    class.bombs()
                ),
                dim,
            );
            surface.set_string(ox + 4, y + 1, class.blurb(), dim);
            y += 3;
        }
        y += 1;
        surface.set_string(
            ox,
            y,
            "Guns: blaster · spread · laser (pierces) · homing · plasma (wide).",
            dim,
        );
        surface.set_string(
            ox,
            y + 1,
            "Kills drop guns, shields, bombs, rapid fire and lives. Every 4th wave is a boss.",
            dim,
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
        if self.game.status == Status::Select {
            match key {
                key!(Left) | key!('h') => {
                    self.pick = (self.pick + ShipClass::ALL.len() - 1) % ShipClass::ALL.len()
                }
                key!(Right) | key!('l') => self.pick = (self.pick + 1) % ShipClass::ALL.len(),
                key!('1') => self.game.start(ShipClass::Interceptor),
                key!('2') => self.game.start(ShipClass::Cruiser),
                key!('3') => self.game.start(ShipClass::Juggernaut),
                key!(Enter) | key!(' ') => self.game.start(ShipClass::ALL[self.pick]),
                _ => {}
            }
        } else {
            match key {
                key!(Left) | key!('h') => self.game.move_ship(-1, 0),
                key!(Right) | key!('l') => self.game.move_ship(1, 0),
                key!(Up) | key!('k') => self.game.move_ship(0, -1),
                key!(Down) | key!('j') => self.game.move_ship(0, 1),
                key!(' ') | key!('f') => self.game.fire(),
                key!('b') => self.game.bomb(),
                key!('p') => self.paused = !self.paused,
                key!('r') => self.retry(),
                key!('n') => self.restart(),
                _ => {}
            }
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
        let enemy_style = theme.get("error");
        let tank_style = theme.get("warning");
        let boss_style = theme.get("error");
        let ship_style = theme.get("function");
        let shot_style = theme.get("warning");
        let beam_style = theme.get("ui.text.focus");
        let eshot_style = theme.get("error");
        let power_style = theme.get("string");

        surface.clear_with(area, bg);
        if area.width < W as u16 + 4 || area.height < H as u16 + 6 {
            surface.set_string(
                area.x,
                area.y,
                &format!("Nova needs a {}×{} window.", W + 4, H + 6),
                text_style,
            );
            return;
        }
        if self.game.status == Status::Select {
            self.render_select(area, surface, ctx);
            return;
        }

        let g = &self.game;
        let ox = area.x + 2;
        let top = area.y + 3;
        surface.set_string(
            ox,
            area.y,
            &format!(
                "NOVA  wave {}  score {}  chain ×{}  [{}]",
                g.wave,
                g.score,
                g.combo,
                g.class.name()
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
        // Boss health bar, on its own line so the court never shifts.
        if let Some(boss) = &g.boss {
            let width = 24i32;
            let filled = (boss.hp.max(0) as i32 * width / boss.max_hp.max(1)) as usize;
            surface.set_string(
                ox,
                area.y + 2,
                &format!(
                    "BOSS {}{}  phase {}",
                    "▰".repeat(filled),
                    "▱".repeat(width as usize - filled),
                    boss.phase()
                ),
                boss_style,
            );
        }

        // Court walls.
        for c in 0..W {
            surface.set_string(ox + c as u16, top, "─", wall_style);
            surface.set_string(ox + c as u16, top + 1 + H as u16, "─", wall_style);
        }
        let cell = |r: i16, c: i16| (ox + c as u16, top + 1 + r as u16);
        let on_board = |r: i16, c: i16| (0..H).contains(&r) && (0..W).contains(&c);

        // The boss hull, widest thing on the court, drawn under everything else.
        if let Some(boss) = &g.boss {
            for dx in -BOSS_HALF..=BOSS_HALF {
                let (r, c) = (boss.pos.0, boss.pos.1 + dx);
                if on_board(r, c) {
                    let (x, y) = cell(r, c);
                    let glyph = if dx == 0 { "◉" } else { "▓" };
                    surface.set_string(x, y, glyph, boss_style);
                }
                if dx.abs() <= 2 && on_board(boss.pos.0 + 1, c) {
                    let (x, y) = cell(boss.pos.0 + 1, c);
                    surface.set_string(x, y, "▀", boss_style);
                }
            }
        }
        for e in &g.enemies {
            let (r, c) = e.pos;
            if on_board(r, c) {
                let (x, y) = cell(r, c);
                let style = if e.kind == EnemyKind::Tank {
                    tank_style
                } else {
                    enemy_style
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
        // The hull itself, blinking while its invulnerability runs out.
        if !g.invulnerable() || self.frames % 6 < 3 {
            let (x, y) = cell(g.ship.0, g.ship.1);
            surface.set_string(x, y, g.class.glyph(), ship_style);
        }

        let status_y = top + 2 + H as u16;
        let status = match g.status {
            Status::Lost => format!(
                "Game over — score {}.  r: same hull  n: new hull  q: quit",
                g.score
            ),
            Status::WaveClear => format!(
                "Wave {} cleared — score {}.  Next wave incoming…",
                g.wave, g.score
            ),
            Status::Playing if self.paused => {
                "Paused — p resume · r retry · n new · q quit".to_string()
            }
            Status::Playing => {
                "←/→/↑/↓ fly · SPC fire · b bomb · p pause · n new · q quit".to_string()
            }
            Status::Select => String::new(),
        };
        surface.set_string(ox, status_y, &status, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cruiser one keypress into wave one, with nothing else on the court.
    fn flying() -> Game {
        let mut g = Game::new(1);
        g.start(ShipClass::Cruiser);
        g.enemies.clear();
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
        for _ in 0..g.class.fire_cadence() {
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
        g.enemy_shots = vec![Shot::enemy((g.ship.0 - 1, g.ship.1), 0, 1)];
        let lives = g.lives;
        g.advance_enemy_shots();
        assert_eq!(g.lives, lives - 1, "the life goes");
        assert_eq!(g.shield, g.max_shield, "shields come back for the next one");
        assert_eq!(g.weapon_level, 2, "and the gun drops a level");
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
        g.damage_ship();
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
    fn every_fourth_wave_is_a_boss_wave() {
        let mut g = flying();
        g.wave = 3;
        g.spawn_wave();
        assert!(g.boss.is_none(), "wave three is a formation");
        g.wave = BOSS_EVERY;
        g.spawn_wave();
        assert!(g.boss.is_some(), "wave four brings the boss");
        assert!(!g.enemies.is_empty(), "with a kamikaze escort alongside it");
    }

    #[test]
    fn the_boss_enrages_as_its_armour_burns_off() {
        let mut boss = Boss {
            pos: (FORMATION_TOP, W / 2),
            hp: 100,
            max_hp: 100,
            dir: 1,
            cooldown: 0,
            minion_timer: 0,
        };
        assert_eq!(boss.phase(), 1);
        boss.hp = 50;
        assert_eq!(boss.phase(), 2, "past a third of the hull it escalates");
        boss.hp = 20;
        assert_eq!(boss.phase(), 3, "and enrages below a third");
        assert_eq!(boss.speed(), 2, "the enraged sweep is twice as fast");
        assert!(boss.cadence() < 14, "and it fires far more often");
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
    fn a_cleared_wave_rolls_into_the_next_with_the_run_intact() {
        let mut g = flying();
        g.score = 4_200;
        g.weapon = Weapon::Plasma;
        g.weapon_level = 3;
        g.shield = 0;
        g.check_end();
        assert_eq!(g.status, Status::WaveClear, "an empty court ends the wave");
        for _ in 0..INTERMISSION_TICKS {
            g.step();
        }
        assert_eq!(g.status, Status::Playing, "the next wave starts itself");
        assert_eq!(g.wave, 2);
        assert!(!g.enemies.is_empty(), "with a fresh formation");
        assert_eq!(g.weapon, Weapon::Plasma, "the gun carries over");
        assert_eq!(g.weapon_level, 3);
        assert_eq!(g.shield, g.max_shield, "shields are topped back up");
        assert!(g.score > 4_200, "and the wave bonus is banked");
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
    fn a_wave_of_stepping_never_panics_or_leaks_state() {
        let mut g = Game::new(7);
        g.start(ShipClass::Interceptor);
        for i in 0..600 {
            if i % 3 == 0 {
                g.fire();
            }
            if i % 97 == 0 {
                g.bomb();
            }
            g.move_ship(if i % 2 == 0 { 1 } else { -1 }, 0);
            g.step();
            assert!(
                (1..W - 1).contains(&g.ship.1),
                "the hull stays on the court"
            );
            assert!(g.shots.len() <= MAX_SHOTS, "shots stay under the cap");
            assert!(
                g.enemies.iter().all(|e| e.pos.0 < H),
                "no hull is left below the floor"
            );
        }
    }
}
