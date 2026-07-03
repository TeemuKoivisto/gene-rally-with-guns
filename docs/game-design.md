# Gene Rally with Guns — Game Design Doc

> Working title. Prototype game built in **Bevy** (Rust).
> Status: **design exploration** (pre-implementation). Sections marked _Recommendation_ are my default proposal; _Open_ marks a decision still to make.

---

## 1. Elevator pitch

A fast, **3D isometric** couch-chaos car-combat party game with a **tiny-Hot-Wheels toy aesthetic** — bright low-poly cars zipping around diorama-scale arenas. Up to **8 players** drop into a small toy city / scrapyard / industrial lot, grab **weapon drops**, and fight to be the **last car running**. Each player gets **one life per level**; die and you're out until the next level. Roaming **police** patrol every arena as a constant hazard — they punish campers and, if you start wrecking them, the **threat escalates**, spawning tougher units. Rounds are short and lethal; the party rolls level to level.

**The one-line hook:** _Last toy car running. Keep moving — the cops don't like campers._

---

## 2. Reference DNA

Each reference contributes one specific thing so we borrow deliberately, not vaguely:

| Reference | What we take |
|---|---|
| **Gene Rally** | Tight, readable arcade driving; minimalist chunky cars; shared-screen local multiplayer. |
| **Micro Machines** | Tiny toy cars at diorama scale; shared camera that fits everyone; elimination pressure. |
| **Hot Wheels Unleashed** | The **modern low-poly toy-car aesthetic** we're matching — candy colors, clean shapes. |
| **TowerFall / Duck Game** | **One life, very fast rounds**, instant restart, best-of-many party structure. |
| **GTA 1 / 2** | Roaming cops and **escalating police response** as flavor (but ours is anti-camp, not leader-seeking). |
| **Liero** | Fast, lethal deathmatch; weapon pickups; destructibility as a toy. |
| **Crash Team Racing** | Weapon-drop crates, item balance, party legibility. |
| **Twisted Metal** | Car-combat archetypes; arena hazards. |

---

## 3. Design pillars

1. **Instantly legible chaos.** Eight toy cars on one screen stay readable: strong per-player colors, clean silhouettes, a quiet map palette so cars pop. If a spectator can't tell who just died, we failed.
2. **Keep moving.** Camping is death. Roaming cops flush out anyone who turtles; the arena is small and lethal. Movement is the default state.
3. **Very fast, one-life rounds.** Target ~**45–90 seconds** per level. Die once and you spectate until the next level loads seconds later. The party's momentum is the product.
4. **Fun-per-effort.** It's a prototype. Every system picks the cheapest version that still delivers the toy (see destructibility §7, arcade handling §9).
5. **Driving feel is sacred.** If a bare toy car doesn't feel great to throw around _before_ any guns exist, nothing else matters.

---

## 4. Core loop

**Moment-to-moment:** drive → grab a drop → fight rivals → dodge patrolling cops → be the last one alive.

**Level (round):** ~45–90s, 8 players enter, **1 life each**, last car running wins the level. Cops patrol throughout; wrecking them escalates the threat. Short and brutal.

**Match:** a rotation of small levels (party-game style). Points per level placement accumulate; most points across the set wins the match. A full match ≈ 8–15 min — one couch session.

```
        ┌───────────────────────────────────────────────┐
        │  DRIVE ──► GRAB DROP ──► FIGHT ──► SURVIVE      │
        │    ▲                                   │        │
        │    │                                   ▼        │
        │  KEEP MOVING ◄──── COPS PATROL ◄── (or camp,    │
        │                     & ESCALATE       and die)   │
        └───────────────────────────────────────────────┘
   Last car running ► score ► next level ► repeat ► match winner
```

### The Cops / Threat system (anti-camp engine)

This is the mechanic that keeps the game moving — spec it as an **anti-camp pressure system**, not a wanted-level-targets-the-leader system.

- **Cops patrol.** Police vehicles roam the arena on patrol/wander routes and engage any player they get near. They are a **constant roaming hazard** — their whole job is to make standing still lethal, so nobody camps a corner with a good weapon.
- **A shared Threat level escalates.** Killing/wrecking cops raises a single **map-wide Threat meter**. Higher threat spawns **more cops and tougher units** (patrol car → SWAT van → helicopter → riot truck). It's shared, not per-player.
- **Risk/reward on cop-killing.** Cops are in your way and killing one feels great — but it makes the arena harder for _everyone_, including you. Do you clear the cop blocking the good pickup, or leave it and stay quiet?
- **Naturally bounded by 1-life rounds.** Because rounds are short and players get eliminated fast, threat can't spiral forever — cops are **mostly spawned at a managed count** that ramps modestly within a ~minute-long level. No runaway swarms.
- **Cops don't win.** They eliminate players and shape the space, but victory is last-_player_-standing. Cops are the environment, not a competitor.

_Optional tuning knob:_ a gentle **time-based threat ramp** (more cops the longer a level runs) as a soft "sudden death" so two survivors can't stalemate. Use only if playtests show rounds dragging.

_Open:_ does wrecking a cop award any **points**, or purely escalate threat? Recommend **no points** (or tiny) — keep the incentive about tactics/space, not farming.

---

## 5. Game modes

**Flagship — Last Car Running:** 8-player FFA elimination, 1 life, last alive wins the level. Cops patrol + escalate. Best-of-N levels = match. **This is what the prototype ships.**

Later variants (all reuse the same core + cops):

| Mode | Idea | Reuses |
|---|---|---|
| **Last Team Standing** | 2v2v2v2 / 4v4 elimination. | Elimination, teams |
| **Smash & Grab** | A loot pickup spawns; carrying it spikes cop attention onto you. Bank it to score, or just survive. | Cops, pickups, zones |
| **Score Attack (chill)** | Respawn-on + timed frags for casual/warmup sessions where nobody's benched. | Scoring, respawn |
| **Cop Rush (co-op)** | All players survive escalating cop waves together. Horde mode. | Cops, threat ramp |

_Recommendation:_ build **Last Car Running** end-to-end for the vertical slice; the rest is content on shipped systems, not new tech.

---

## 6. Players, input & camera

Target: **up to 8 local players**, gamepad-first. Biggest UX constraint; shapes camera, art, HUD.

- **Input:** one gamepad per player; keyboard for 1–2 as dev/fallback. Per-player action maps (§9). A **lobby/press-to-join** screen assigns each controller a color/car.
- **Camera — single shared isometric view.** No split-screen at 8. A **shared iso camera dynamically zooms to fit all _live_ cars**:
  - fixed isometric angle (see §9/§10), zoom-to-fit the live players' bounding box + margin,
  - a **min-zoom floor** so toy cars never shrink below readable size,
  - **clamped to arena bounds** (arenas are small, diorama-scale, fully bounded),
  - **tightens naturally as players are eliminated** — a nice side effect of 1-life: fewer live cars → closer, tenser framing toward the finish.
- **Spread handling:** arenas are deliberately small enough to fit at max zoom, so this mostly solves itself via level design. Soft-leash chip damage near the edge is the backup if needed.

_Recommendation:_ small bounded diorama arenas + shared iso zoom-to-fit + min-zoom floor. Design arenas **to the camera**, not the reverse.

---

## 7. Destructible terrain (light / tactical)

Chosen scope: **light / tactical** — the drivable surface stays stable, but the world has stuff you can blow apart. Deliberately **not** Liero-style per-pixel destruction (expensive, fights rigid-body physics, wrong effort/payoff for a prototype).

**Approach: object-based destruction, now in 3D.**

- Destructibles are **discrete glTF entities** with HP: crates, fences, market stalls, dumpsters, parked cars, **explosive barrels / gas pumps** (chain reactions!), light poles, toy-scale props.
- On destroy: remove collider → spawn debris/particles → maybe open a new path. Clean fit with the 3D physics engine (despawn a rigid body).
- **Scorch marks & craters = decals** on the ground mesh (cosmetic; no collision change). Cheap, sells mayhem.
- Explosive props are the fun multiplier: shoot the gas pump next to a rival, or use it to block a chasing cop.

_Optional stretch:_ chunked **destructible walls** (a wall = a row of collider chunks; blow a hole for a shortcut). Only if the object layer proves fun. Not in the first slice.

_Why this is right:_ ~80% of the "I blew up the map" fantasy for ~20% of the cost, and it never fights physics.

---

## 8. Weapons, pickups & vehicles

### Pickups (CTR/Liero style)

- **Drop crates** spawn at fixed points; respawn on a timer. Drive over to grab.
- **Slot model** (_Open_): recommend a **single active-weapon slot** (grab overwrites), maybe + one **utility slot** (mine/oil/boost). Two full inventories per player is unreadable in a crowd of 8.
- With 1 life, drops are **high-stakes** — grabbing that rocket is worth breaking cover for.

### Weapon starter set

| Weapon | Type | Feel |
|---|---|---|
| **Machine gun** | Front-mounted, ammo-limited | Bread-and-butter chip damage. _(First weapon in the slice.)_ |
| **Homing rocket** | Lock-on projectile | Punishes runners; satisfying. |
| **Mines** | Drop behind | Zone denial, comedy deaths. |
| **Oil slick** | Drop behind | Non-lethal disruption; spins cars out. |
| **Shotgun** | Short cone | High-skill close brawls. |
| **Mortar / lob** | Arced AoE | Lob over walls; rewards positioning. |
| **EMP** | Radius stun | Anti-swarm; stuns cops too. |
| **Nitro / boost** | Utility | Escape, ram, reposition. |
| **Shield / repair** | Utility | Defensive counterplay. |

_Balance note:_ frequent drops + limited ammo keeps power flowing without snowballing. Weapons do double duty against players _and_ cops — clearing a patrol or shaking a chopper.

### Vehicles

- Prototype: **one arcade toy car**, differentiated only by **player color**.
- Later: 2–4 archetypes on a **speed / armor / handling** triangle (nimble kart, armored van, muscle car). Pure preference — no hard unlocks needed for a party game.

### Cop / threat roster (the environment)

| Unit | Spawns at threat | Behavior |
|---|---|---|
| **Patrol car** | Base | Roams/wanders; engages & rams nearby players. The anti-camp workhorse. |
| **SWAT van** | Mid | Tankier; drops spike strips / roadblocks. |
| **Helicopter** | High | Aerial spotlight + fire; can't be rammed. |
| **Riot truck / "the tank"** | Max | Slow, terrifying, hilarious finale unit. |

Cops are a **shared hazard**: wreck the patrol blocking a pickup, bait a cop into a rival, or lead a chase into a gas pump. But every cop you kill raises the threat for the whole arena.

---

## 9. Technical architecture (Bevy)

### Stack — recommendations

| Concern | Pick | Notes / alternative |
|---|---|---|
| Engine | **Bevy**, current stable (0.16/0.17 series) | **Pin the version.** Every ecosystem crate must match the Bevy version — #1 source of build pain. Budget an hour day one. |
| Dimensionality | **3D**, isometric camera, low-poly toy assets | Confirmed direction. Slightly more asset/handling work than 2D; buys the Hot-Wheels look. |
| Camera | **Orthographic**, fixed iso angle | Clean toy-diorama look + easy zoom-to-fit. _Alt:_ narrow-FOV **perspective** for a cuter tilt-shift depth — decide by taste (§12). |
| Physics | **Avian3d** (ECS-native, ex-`bevy_xpbd`) | _Alt:_ `bevy_rapier3d` (mature). Either works. |
| Vehicle model | **Simplified arcade**, not a raycast-suspension sim | Rigid body ~constrained to the ground plane + scripted drive/steer/lateral-grip forces. Only go full raycast-vehicle if we want real ramps/jumps. Keeps feel tunable. |
| Input | **`leafwing-input-manager`** | Per-player action maps for 8 gamepads; device-agnostic actions. |
| Assets | glTF low-poly (e.g. **Kenney car/city kits** as prototype placeholders) | Free, on-aesthetic, instantly usable; replace later. |
| Audio | Bevy audio or **`bevy_kira_audio`** | Kira for positional SFX / better mixing. |

### ECS structure — plugins

```
GameApp
├── CorePlugin        // states, config, fixed-timestep
├── InputPlugin       // leafwing action maps, gamepad→player assignment
├── PhysicsPlugin     // avian3d setup, collision layers
├── VehiclePlugin     // arcade driving (plane-constrained forces)
├── WeaponPlugin      // firing, projectiles, damage
├── PickupPlugin      // crate spawn/respawn, collection
├── ThreatPlugin      // shared threat meter, cop spawn director
├── CopAiPlugin       // patrol/engage steering state machine
├── DestructionPlugin // destructible HP, debris, decals
├── CameraPlugin      // iso zoom-to-fit + bounds clamp
├── HudPlugin         // alive-players, scores, threat meter
├── GameFlowPlugin    // lobby → level → level-end → next → match-end
├── MapPlugin         // arena load, spawn points, hazards, patrol routes
└── AudioPlugin       // engines, sirens, weapons, dynamic music
```

### States

`MainMenu → Lobby (join/assign controllers) → Loading → InLevel → LevelEnd → (rotate) → MatchEnd`

### Key components (sketch)

`Player{id,color}`, `Vehicle{stats}`, `Alive`/`Eliminated`, `Health`, `WeaponSlot{kind,ammo}`, `Projectile`, `Destructible{hp}`, `Pickup{kind}`, `CopUnit{kind,state}`, `SpawnPoint`, `PatrolRoute`, `Hazard`. Shared resource: `Threat{level}`.

### Key events

`DamageEvent`, `EliminationEvent`, `WeaponFiredEvent`, `PickupCollectedEvent`, `CopWreckedEvent`, `ThreatChangedEvent`, `LevelEndedEvent`.

### Notes on the hard bits

- **3D arcade handling:** treat the car as a rigid body pinned near the ground plane; apply forward drive force, yaw from steering, and strong lateral friction for grip (bleed it for drift). This is far simpler and more tunable than a real suspension/raycast vehicle, and gives the toy-car feel. This is the **M1 gate** — nail it before anything else.
- **Iso camera fit:** each frame, compute the world AABB of live cars, project onto the camera plane, set orthographic scale (or camera distance) to fit + margin, clamp to `[minZoom, arenaBounds]`, smooth-lerp. Small self-contained system; fewer live cars → tighter shot for free.
- **Cop AI:** no built-in nav in Bevy. Small arenas → **steering behaviors** (wander/patrol + seek/pursue + raycast avoidance) and a tiny state machine (`Patrol → Engage → Retreat`). Deliberately dumb and telegraphed; legible beats smart. Nav-grid is a later upgrade.
- **Threat director:** a system watching `Threat` that maintains a target cop population and unit mix; spawn/despawn to hit it. Bounded because levels are short.
- **Fixed timestep:** run driving + physics + weapons on `FixedUpdate` for stable feel across framerates.

---

## 10. Art & audio direction

- **View:** **3D isometric**, fixed camera angle, diorama scale — like a Hot Wheels play set seen from above at a tilt. Optional subtle **tilt-shift / depth-of-field** to sell the "tiny toys" fantasy.
- **Aesthetic:** modern low-poly, **candy-bright saturated** colors, clean rounded shapes, chunky readable silhouettes. Cars read as glossy die-cast toys.
- **Legibility first:** 8 distinct **player colors** on car bodies + a small floating **indicator ring/arrow** per car. The map is desaturated relative to the cars so players always pop. Cops are unmistakable (black/white + flashing lights).
- **Tone:** loud, comedic toy-scale mayhem — not gritty. Saturday-morning chaos with sirens.
- **Audio:** chunky engine loops, punchy weapon SFX, **sirens that swell with the threat level**, and **dynamic music** that intensifies as cops escalate and as a level nears its last survivors. A blindfolded player should feel the threat rising.

---

## 11. Prototype scope — the thin vertical slice

Chosen milestone: **thin full loop** — touch every core system once, shallowly, and prove the whole thing is fun end-to-end before deepening any part. Now with **3D iso** and **1-life elimination**.

**Slice contents:**

- [ ] **1 small 3D iso arena** (toy city block — roads, buildings-as-walls, a couple alleys), fully bounded.
- [ ] **2–8 low-poly cars** via gamepads; great arcade driving on the plane; lobby press-to-join.
- [ ] **Shared iso dynamic-zoom camera** with min-zoom floor + bounds clamp.
- [ ] **1 weapon** (machine gun) + **1 pickup crate** that grants it/ammo.
- [ ] **Health + 1-life elimination:** die once → spectate. **Last car running wins the level.**
- [ ] **Level rotation:** on level end → score → load next level (may reuse the same arena in the slice) → repeat → **match winner** after N.
- [ ] **Cops, minimal:** 1–2 **patrolling** cops that can eliminate you; wrecking one raises **Threat** and spawns another/tougher unit.
- [ ] **1 destructible object** (crates/barriers) + **1 explosive prop** (barrel) for chain-reaction fun.
- [ ] **HUD:** who's alive, threat meter, match score; **level-end screen**.

**This slice proves:** driving + shooting + pickups + patrolling-cop/threat + destruction + 1-life elimination + level rotation all work together and are fun. Everything after is content and depth on shipped systems.

### Build order (each step playable)

| Milestone | Delivers |
|---|---|
| **M0 — Scaffold** | Bevy 3D project, pinned deps, iso camera, fixed-timestep, flat arena, one controllable car. |
| **M1 — Driving + camera** | Great arcade handling (the plane-force model); 2–8 cars; iso zoom-to-fit; lobby join. _(Driving feel is sacred — stop and tune here.)_ |
| **M2 — Combat + pickups** | Machine gun, projectiles, damage, **1-life elimination + last-car-running**, crate pickup. |
| **M3 — Cops + threat** | Patrolling cop(s), threat meter, escalation spawn, HUD threat bar. |
| **M4 — Destruction** | Destructible crates + explosive barrels + debris/decals. |
| **M5 — Game flow** | Level rotation, placement scoring, level-end/match-end, minimal menus. |
| **M6 — Feel pass** | Screenshake, SFX, particles, siren/music intensity, toy-car polish, tilt-shift. |

After M6 = a genuinely playable party toy. Then widen: more weapons, cop tiers, arenas, modes.

---

## 12. Open decisions (resolved marked ✓)

1. ✓ **3D isometric, low-poly Hot-Wheels aesthetic** (was 2D/3D open).
2. ✓ **1 life per level, elimination, last-car-running**; fast level rotation (was respawn/elim open).
3. ✓ **Cops = anti-camp patrols + shared escalating threat**, bounded by short rounds (was wanted-level-targets-leader).
4. **Orthographic vs slight-perspective** iso camera — _lean orthographic_ for legibility; perspective for cuter depth. Try both cheaply in M1.
5. **Weapon slots** — _recommend single active slot (+ optional utility)_ for legibility.
6. **Target round length** — _recommend 45–90s_; confirm in playtest.
7. **Cop kills: points or pure escalation?** — _recommend pure escalation_ (no/low points).
8. **First arena setting** — city block / scrapyard / industrial? _Recommend **toy city block**_ (best showcases roads + patrols). Pick one to build.
9. **Working title** — candidates: _Gene Rally with Guns_ (keep), _Roadkill_, _Diecast_, _Scale Model_, _Last Car Running_, _Copbait_, _Pileup_, _Gridlock_.

---

## 13. Risks & unknowns

| Risk | Mitigation |
|---|---|
| Bevy ecosystem version drift breaks the build | Pin a known-compatible set (Bevy + Avian3d + leafwing) day one; upgrade deliberately. |
| 3D arcade handling feels floaty/wrong | Use the simple plane-force model; M1 gate — don't proceed until a bare toy car is fun. |
| 8-player readability collapses into soup | Legibility is pillar #1: color-coding + indicator rings, min-zoom floor, quiet map palette, quiet cops-are-loud contrast; playtest at full 8 early. |
| 1-life rounds feel punishing / dead time | Keep levels ~<90s and load the next in seconds; eliminated players spectate briefly, not for minutes. |
| Cops feel dumb or unfair | Keep them simple + telegraphed; they're a _hazard_, not a boss. Legible beats smart. |
| Destructibility scope-creeps toward Liero pixels | Hard-hold object-based scope; pixel destruction is explicitly out for the prototype. |
| 3D asset pipeline slows iteration | Use free Kenney low-poly kits as placeholders; art polish waits until M6. |
| Feature breadth before the core is fun | Thin slice first; content only after M6 proves the toy. |

---

_Next step after sign-off: lock the remaining §12 opens (mainly camera projection, first arena, title), then scaffold M0 — a Bevy 3D project with a pinned dep set, an iso camera, and one drivable toy car._
```

