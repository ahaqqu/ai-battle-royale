.PHONY: build viewer serve test ci

# Native sim + runner
build:
	cargo build --release

# WASM sim + web viewer into viewer/dist
viewer:
	wasm-pack build crates/gunbatte-wasm --target web --release --out-dir ../../viewer/src/wasm
	cd viewer && npm install && npm run build

serve: build viewer
	./target/release/gunbatte-runner serve --port 8321

demo: build
	./target/release/gunbatte-runner run --preset default16 --seed 42 --out replays/demo.json

test:
	cargo test --workspace
	cargo test --release --workspace

ci: test
	cargo clippy --workspace --all-targets -- -D warnings
