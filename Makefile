.PHONY: run build

run:
	@v=$$(cat build_version); v=$$((v + 1)); echo $$v > build_version; echo "build #$$v"
	cargo run -- --empty

build:
	@v=$$(cat build_version); v=$$((v + 1)); echo $$v > build_version; echo "build #$$v"
	cargo build
