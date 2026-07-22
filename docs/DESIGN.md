# MOE TRANSIT ARCADE — design document

**codename:** `cybernetic-army`
**one-liner:** instakill boomer shooter in a melting low-poly dream, where the level breathes to the music.

---

## 1. Vision & pillars

1. **Instant death, instant respawn.** Every weapon kills in one hit (Quake instagib lineage). No health bars, no loadouts, no progression. The skill ceiling is movement and aim; the floor is "my friend clicked the link 10 seconds ago and is already playing."
2. **The level is a music visualizer.** Geometry, palette, fog, and skybox react to the soundtrack (moe transit authority tracks) in real time. The audio isn't background — it's the environment's heartbeat.
3. **Dream logic, not realism.** LSD Dream Emulator / PS1 aesthetic: low-poly, vertex snapping, affine texture warp, fog that hides the seams, sudden palette shifts. Wrong on purpose.
4. **Zero friction.** Runs in the browser from a link on moetransitauthority.com. No install, no account. Join a match in two clicks.

## 2. Aesthetic spec

- **Geometry:** < 500 tris per prop, < 15k tris per arena. Chunky, unsmoothed normals.
- **PS1 rendering quirks (shader-emulated):**
  - vertex snapping to a virtual low-res grid (jittery vertices)
  - affine (non-perspective-correct) texture mapping on select surfaces
  - hard-edged dithered fog, short draw distance
  - low-res render target (~320×240 internal, integer-upscaled) — also our perf budget cheat
- **Palette:** miku teal core (`#137a7f`, `#86cecb`) + dream-shift palettes (sickly pink, ochre, void purple) that rotate on song sections.
- **Audio-reactive layers** (see §6): bass → vertex displacement amplitude / fog pulse; mids → palette lerp; highs → skybox sparkle/noise; beat events → geometry "breathing", light flicker.

## 3. Gameplay spec

- **Mode 1 (MVP):** free-for-all instagib, 2–4 players, first to 15 frags or 5 minutes.
- **Weapon:** single hitscan railgun. 1.2s cooldown. Kill = 1 frag. Miss = vulnerability window. A short projectile-visual beam renders for feedback (hitscan under the hood for determinism).
- **Movement:** air-strafe + bunny-hop friendly (Quake-style accel), jump pads, no fall damage. Speed is survival.
- **Respawn:** instant, at the spawn point furthest from your killer.
- **Arenas:** small (30s to cross), vertical, wrap-around teleporters. One arena per track; arena mood = track mood.
- **Sessions:** matches are rooms of 2–4. Solo visitors get a target-practice dream (same arena, floating targets on the beat) so the link is never dead.
- **Mode 2: RHYTHM MODE.** Same arena, same railgun — but the gun is only *live* in a window around each beat (±1 sim tick tuned per track, ~±80ms). Off-beat shots fizzle with a sad visual. Consecutive on-beat frags build a combo that shortens the beam cooldown; dropping the combo resets it. Respawns quantize to the next bar line. Firefights become rhythm duels: everyone shares the same deterministic beat clock (`beats.json` + sim tick — see §5), so "was that on beat?" has one answer on every peer. Works in FFA and solo target practice.

## 4. Tech stack

| Layer | Choice | Why |
|---|---|---|
| Engine | **Bevy 0.16** (Rust) | ECS fits rollback netcode; first-class wasm support |
| Target | `wasm32-unknown-unknown`, WebGL2 baseline | WebGL2 = works everywhere today; WebGPU as progressive upgrade later |
| Bundler | **Trunk** | zero-config wasm build + dev server for Bevy |
| Netcode | **bevy_ggrs** (rollback) + **bevy_matchbox** (WebRTC p2p) | the proven browser-multiplayer stack for Bevy; P2P = no game servers to run |
| Signaling | **matchbox_server** on Fly.io/Railway free tier | tiny stateless WebSocket service; only makes introductions, relays nothing |
| Audio in | **Web Audio API** `AnalyserNode` via `web-sys` (wasm), `cpal`+FFT fallback (native dev) | FFT bands each frame → shader uniforms |
| Music sync | offline beat/section analysis (librosa script) → `beats.json` per track | precomputed beats are deterministic & free at runtime; FFT is for continuous texture |
| Hosting | GitHub Pages (this repo) at **arcade.moetransitauthority.com** | same free static pipeline as the main site |

## 5. Netcode design (the hard part, decided early)

**Model:** deterministic lockstep with rollback (GGRS). All peers simulate the full game; only *inputs* cross the wire (~a few bytes/frame). WebRTC unreliable datachannels via matchbox.

- **Determinism contract:** the entire simulation (movement, cooldowns, hit resolution, spawns) runs in a fixed-timestep `GgrsSchedule`, touching only rollback-registered components. Rendering, audio, shaders, and particles live outside the contract and may do whatever they want.
- Same wasm binary on all peers ⇒ float determinism is a non-issue in practice (identical codegen). Native↔wasm cross-play is explicitly out of scope.
- **Rollback budget:** 2–4 players, input delay 2 frames, max rollback 8 frames. The 320×240 render budget leaves plenty of CPU headroom for resimulation.
- **Hitscan + rollback = favor-the-shooter for free.** The shot resolves in the simulated past, so what you saw is what you hit. This is why the weapon is hitscan.
- **Matchmaking:** matchbox rooms. `next_2` / `next_4` quick-match rooms + shareable private room codes (`arcade.moetransitauthority.com/#room=xyz`). No accounts, no lobby service, no persistence.
- **Music in multiplayer:** the track is a *shared clock*, started at a synchronized sim frame, so all players see the same breathing world. Beat-spawned pickups derive from `beats.json` + sim frame (deterministic); FFT visuals are local-only cosmetics.

## 6. Audio-reactive shader pipeline

```
        <audio> element (track stream)
              │
    Web Audio AnalyserNode (fft_size 1024)
              │  every frame
    JS/web-sys: freq data → 4 band energies (bass/lowmid/highmid/treble)
              │
    Bevy resource: AudioBands { bass, lowmid, highmid, treble, beat_t }
              │
    custom Material extension (WGSL uniforms)
              ├── vertex stage: displacement = f(bass), vertex snap grid
              └── fragment stage: palette lerp = f(mids), fog pulse, dither
```

- One `ArenaMaterial` (extends `StandardMaterial` via Bevy's material-extension API) used by all world geometry; props share it for cohesion.
- `beat_t` = seconds since last beat (from `beats.json`), drives discrete events (light flash, target spawn) where raw FFT is too mushy.
- Native dev builds read the same bands from cpal+rustfft so shader work doesn't require a browser.

## 7. Module layout (planned)

```
crates/
  game/           # bin — app wiring, states (Menu, Lobby, Match, Dream)
  sim/            # deterministic core: movement, weapons, frags (rollback-safe, no rendering)
  netplay/        # matchbox session, ggrs config, input encoding
  vibe/           # audio bands, beat clock, arena material + WGSL shaders
  arenas/         # arena definitions/loaders (start: hand-coded geometry)
assets/
  tracks/         # ogg music + beats.json (git-lfs if needed)
  textures/
web/              # index.html shell, Trunk config, canvas/CSS
docs/             # this file, ROADMAP.md, decisions/
```

`sim` having zero render/audio deps is what keeps rollback honest — CI can run it headless and hash game states for determinism tests.

## 8. Constraints & budgets

- **Wasm size:** ≤ 20 MB uncompressed (Bevy baseline ~10–15 MB with `opt-level = "z"` + `wasm-opt`; Pages gzips). Load screen with progress bar is mandatory either way.
- **Perf:** 60 fps on an M1 Air / mid Android in Chrome & Firefox. The 320×240 internal target does the heavy lifting.
- **Music licensing:** all tracks are the artist's own — no issue, but keep tracks as streamed assets (not baked into wasm).
- **Site integration:** the arcade opens as a "cabinet" window/tab on moetransitauthority.com linking out to (or iframing) `arcade.moetransitauthority.com`. Standalone page is the primary target; iframe is a bonus.

## 9. Out of scope (v1)

Accounts, rankings, server-authoritative anti-cheat (P2P instagib among friends — cheating is socially policed), >4 players, native releases, mobile touch controls (view-only fallback OK), voice chat.

## 10. Open questions

1. Which track ships with arena #1? (drives palette + beat map first pass)
2. Room discovery: public quick-match at launch, or private room codes only?
3. WebGPU: adopt when Safari share justifies dual-path testing, or WebGL2-only until v2?
4. Does `arcade.moetransitauthority.com` DNS get set up at scaffold time or at first playable?
