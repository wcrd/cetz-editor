# List available recipes
default:
    @just --list

# Install web dependencies
setup:
    pnpm --dir web install

# Build the WASM modules (parser, compiler worker) into web/src/lib/wasm
wasm:
    cargo build -p cetz-wasm -p cetz-worker --target wasm32-unknown-unknown --release
    wasm-bindgen --target web --out-dir web/src/lib/wasm target/wasm32-unknown-unknown/release/cetz_wasm.wasm
    wasm-bindgen --target web --out-dir web/src/lib/wasm target/wasm32-unknown-unknown/release/cetz_worker.wasm

# Run the editor dev server
dev: wasm
    pnpm --dir web dev

# Build the editor for production into web/dist
build: wasm
    pnpm --dir web build

# Run Rust tests and type-check the web app
test: wasm
    cargo test
    pnpm --dir web check

# Render fixtures to generated/ (all, or the named ones)
render *names:
    scripts/render-fixtures.sh {{names}}
