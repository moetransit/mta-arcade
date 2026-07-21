# ROADMAP

Each phase ends in something runnable. Ship ugly, ship often.

## Phase 0 — scaffold ✅ *(this PR)*
Bevy app compiles natively **and** to wasm; a low-poly teal scene spins in the browser via Trunk; CI enforces fmt/clippy/wasm-build on every PR.
- **exit:** `trunk serve` shows the dream; CI green.

## Phase 1 — the dream walk
First-person controller (mouse look, WASD, jump, Quake-style air accel), one hand-built arena with PS1 shader pass #1 (vertex snap, fog, low-res render target).
- **exit:** it *feels* like walking in LSD Dream Emulator at 60fps in-browser.

## Phase 2 — the vibe
Audio pipeline: track playback, AnalyserNode → `AudioBands` resource → arena material uniforms. Offline beat-map script (`tools/beatmap.py`) → `beats.json`; world breathes on bass, palette shifts on sections.
- **exit:** arena #1 visibly dances to track #1 with sound on.

## Phase 3 — the gun
Railgun: hitscan ray, cooldown, beam VFX, frag counter, floating dream-targets that spawn on beats. This is the single-player "target practice dream" — and the sim crate quietly becomes deterministic (fixed timestep, sim/render split) in prep for netplay.
- **exit:** solo loop is fun for 5 minutes; `sim` crate passes a determinism hash test in CI.

## Phase 4 — ghosts in the arena
Multiplayer: matchbox signaling (deploy `matchbox_server` to Fly.io), bevy_ggrs rollback over WebRTC, 1v1 via shared room code. Kills, respawns, frag limit, scoreboard.
- **exit:** two browsers on different networks finish a 1v1 to 15 frags without desync.

## Phase 5 — it's a place now
2–4 player rooms, quick-match rooms, join links (`#room=`), second arena + second track, spawn-furthest logic, kill feed, round flow (countdown → match → results → rematch).
- **exit:** 4-player FFA holds up through a full playlist rotation.

## Phase 5½ — rhythm mode
The beat clock is already deterministic in the sim (Phase 2/3), so this is a rules layer: on-beat fire windows, off-beat fizzle, combo cooldown scaling, bar-quantized respawns. Mode toggle in room setup.
- **exit:** a rhythm-mode 1v1 feels like a fighting game set to the track; no desyncs (beat verdicts identical on both peers by construction).

## Phase 6 — launch
`wasm-opt` size pass + load screen, GitHub Actions deploy to Pages, `arcade.moetransitauthority.com` DNS + CNAME, arcade cabinet section on the main site, playtest party via Discord.
- **exit:** stranger clicks link on the site, is fragging within 30 seconds.

## Phase 7+ — dreams (backlog)
More arenas/tracks, spectator drift-cam (LSD-style idle wander), mobile touch, WebGPU path, cosmetic unlock graffiti, in-world bandcamp jukebox, rhythm-mode leaderboard ghosts.

---

### Sequencing rationale
- Feel (1) before netcode (4): rollback amplifies whatever feel exists — including bad feel.
- Audio (2) before gun (3): the vibe is the product; the gun is the excuse.
- Determinism debt is paid in Phase 3 while the sim is still small — retrofitting it after Phase 5 would be a rewrite.

### Standing risks
| Risk | Mitigation |
|---|---|
| Bevy wasm binary too fat | size budget in CI from Phase 0; `opt-level="z"`, `wasm-opt`, strip default features as needed |
| WebRTC blocked on some networks | matchbox TURN fallback config; document "use a phone hotspot" for the stubborn 5% |
| Rollback desync | state-hash checks between peers in debug builds from Phase 4 day one |
| Bevy minor-version churn | pin exact versions; upgrade as a dedicated PR, never mid-phase |
