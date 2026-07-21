# moe transit arcade

instakill boomer shooter in a melting low-poly dream. bevy + wasm, playable in browser, set to [moe transit authority](https://moetransitauthority.com).

- [design doc](docs/DESIGN.md)
- [roadmap](docs/ROADMAP.md)

## dev quickstart

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk

trunk serve --open        # browser dev loop (primary)
cargo run                 # native window (needs libasound2-dev libudev-dev on linux)
```

## repo law

- `main` is always green; work lands via PR with CI (fmt, clippy -D warnings, wasm build).
- the deterministic sim never touches rendering/audio — see design doc §5 before touching gameplay code.
- bevy version is pinned; upgrades are their own PR.
