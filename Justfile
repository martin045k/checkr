watch-wasm:
    cd wasm; watchexec -w .. -e rs "wasm-pack build --dev --target bundler"

watch-web:
    cd ui; npm run dev

typeshare:
    cd wasm; typeshare . --lang=typescript --output-file=../ui/src/wasm-types.ts
    cd api; typeshare . --lang=typescript --output-file=../ui/src/api-types.ts

build-wasm:
    cd wasm; wasm-pack build --release --target bundler

build-ui: build-wasm typeshare
    cd ui; npm i && npm run build

build-api: build-ui
    cargo build -p api --release

serve-api: build-ui
    RUST_LOG=debug cargo run -p api wup-wup ./FsLexYacc-Starter

# x86_64-apple-darwin
# x86_64-pc-windows-msvc
# x86_64-unknown-linux-gnu
# aarch64-apple-darwin
update-api: build-api
    cp $(which api) FsLexYacc-Starter/dev
