.PHONY: run build test test-vst3

run:
	@v=$$(cat build_version); v=$$((v + 1)); echo $$v > build_version; echo "build #$$v"
	cargo run --bin layers -- --empty

build:
	@v=$$(cat build_version); v=$$((v + 1)); echo $$v > build_version; echo "build #$$v"
	cargo build

test:
	cargo test

test-vst3:
	cargo run --bin test_vst3
