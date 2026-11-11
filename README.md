# BEARACADE 2000

A 3D open-world Rust take on [Bearacade](https://github.com/rorticus/bearacade): an endless
procedurally generated low-poly world full of bears, and one very bouncy moving truck.

```bash
cargo run
```

The first build compiles Bevy and takes a while; after that it starts in seconds.

## Playing

Hit as many bears as you can before the fuel runs out. Scoring follows the original:
350 a bear, chained hits build a combo (MULTIBEAR at 3, ULTRABEAR at 6, UNBEARLEVABLE at 9).
New in 3D: bonus points for long bear tosses, hitting a bear that is still airborne, big air,
flips, barrel rolls, and smashing through small trees.

| Key | Action |
| --- | --- |
| W A S D / arrows | Drive |
| Space | Handbrake (powerslides) |
| Shift | Boost (drinks fuel) |
| W/S, A/D, Q/E in the air | Pitch, spin, roll |
| R | Flip back onto the wheels |
| F | Trade 8000 points for fuel |
| H / C / M | Horn / look back / mute |
| F12 | Screenshot |
| Enter | Start / restart |

Fuel comes from the red cans under the orange beacons, from every bear you hit, and from `F`.

## Web build

```bash
./tools/build_web.sh serve
```

builds a WebAssembly/WebGL2 version into `dist/` and serves it on http://localhost:8080.
`dist/` is plain static files, so it can be hosted anywhere. One-time setup:

```bash
rustup target add wasm32-unknown-unknown
```

```bash
cargo install wasm-bindgen-cli --version 0.2.130 --locked
```

(`wasm-bindgen-cli` must match the `wasm-bindgen` version in `Cargo.lock`; `brew install binaryen`
adds `wasm-opt`, which the script uses to shrink the download if present.) On the web the
options below go in the URL instead: `?seed=42`, `?autopilot`, `?fuel=10`.

Deploying: `wrangler.jsonc` publishes `dist/` as a Cloudflare Worker with static assets, so
after a build `npx wrangler deploy` updates https://bearacade-2000.rorticus.workers.dev.
The wasm file must stay under Cloudflare's 25 MiB per-file limit (it is about 23 MiB).

## Options

- `--seed N` replays a world (the seed is printed at startup and shown on the title screen).
- `--autopilot` / `--autopilot-straight` run a 23 second self-driving smoke test that saves
  screenshots to `shots/` (`--shots DIR` to change) and prints position, score and FPS.
- `--fuel N` starts with a different amount of fuel.

## How it is built

- **Engine**: Bevy 0.16. No physics crate: `src/truck.rs` is a small custom rigid body with four
  soft suspension rays, a friction-circle tyre model, air control and generous arcade assists.
- **World** (`src/terrain.rs`, `src/props.rs`): layered noise height field, flat-shaded and
  streamed in 64 m chunks; meadow, forest, desert and snowy mountain biomes; trees, rocks,
  barrel piles, fuel cans and drivable ramps scattered deterministically per chunk.
- **Bears** (`src/bears.rs`): idle, wander, dance, flee or charge; launched bears become bouncy
  physics bodies, lie dazed for a bit, then get up again.
- **Assets**: the bear and truck models, bear animations, engine/impact sounds and music were
  generated with Ludo (sources in `art/`). The announcer and bear-hit sounds are from the
  original game. `tools/merge_anims.py` merges Ludo's per-clip animation GLBs into
  `assets/models/bear.glb`.
