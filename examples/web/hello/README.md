# tui-lipan web hello

Proof-of-concept: a counter UI running in the browser via xterm.js. Demonstrates
the `web` backend feature - the same `Component` code that runs natively also
compiles to `wasm32-unknown-unknown` and renders through xterm.js with no
changes inside the component itself.

For the full guide see [`docs/web-backend.md`](../../../docs/web-backend.md).

## Prerequisites

- [wasm-pack](https://rustwasm.github.io/wasm-pack/installer/)
- Node (for npm - only needed to install xterm from node_modules)

## Build and run

```bash
cd examples/web/hello
npm install
wasm-pack build --target web
python3 serve.py
# open http://localhost:8080
```

Or from the repo root:

```bash
make -C examples/web hello
```

`serve.py` is required - plain `python -m http.server` serves `.mjs` and
`.wasm` as `text/plain`, which browsers refuse to execute as modules / wasm.

## Fast iteration

```bash
cargo check --target wasm32-unknown-unknown
```

Optional watch loop:

```bash
cargo watch -w ../../.. -w src -s 'wasm-pack build --target web'
```

Checks the full wasm32 compile without the wasm-pack link step.

## Optional size optimization

After `wasm-pack build`, run:

```bash
wasm-opt -O2 -o pkg/tui_lipan_web_hello_bg.wasm pkg/tui_lipan_web_hello_bg.wasm
```

This usually reduces wasm size significantly for release/dev sharing builds.

## Controls

| Key | Action |
|-----|--------|
| `+` / `=` | Increment counter |
| `-` / `_` | Decrement counter |
| Resize window | Reflows layout |

## Why a separate workspace?

This crate uses `crate-type = ["cdylib"]` and `--features web`, which must not
leak into the root workspace's native `cargo check`. The `[workspace]` table in
this `Cargo.toml` declares it a standalone workspace, so `cargo check
--workspace` at the repo root never builds it.
