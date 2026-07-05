# tui-lipan web search_palette

Browser/WASM port of `examples/search_palette_hub.rs` (Palette tab) using xterm.js and the `web`
backend.

For the full guide see [`docs/web-backend.md`](../../../docs/web-backend.md).

## Prerequisites

- [wasm-pack](https://rustwasm.github.io/wasm-pack/installer/)
- Node (for npm - only needed to install xterm from `node_modules`)

## Build and run

```bash
cd examples/web/search_palette
npm install
wasm-pack build --target web
python3 serve.py
# open http://localhost:8080
```

Or from the repo root:

```bash
make -C examples/web search-palette
```

`serve.py` is required - plain `python -m http.server` serves `.mjs` and
`.wasm` as `text/plain`, which browsers refuse to execute as modules / wasm.

## Optional size optimization

After `wasm-pack build`, run:

```bash
wasm-opt -O2 -o pkg/tui_lipan_web_search_palette_bg.wasm pkg/tui_lipan_web_search_palette_bg.wasm
```

This usually reduces wasm size significantly for release/dev sharing builds.

## Fast iteration

```bash
cargo check --target wasm32-unknown-unknown
```

Optional watch loop:

```bash
cargo watch -w ../../.. -w src -s 'wasm-pack build --target web'
```

## Controls

| Input | Action |
|---|---|
| `/` | Open search palette |
| `Esc` | Close palette |
| `↑`/`↓` | Move selection |
| `Enter` | Activate selected item |
| `t` or `Ctrl+t` (while open) | Toggle transparent modal frame |

## Why a separate workspace?

This crate uses `crate-type = ["cdylib"]` and `--features web`, which must not
leak into the root workspace's native `cargo check`. The `[workspace]` table in
this `Cargo.toml` declares it a standalone workspace, so `cargo check
--workspace` at the repo root never builds it.
