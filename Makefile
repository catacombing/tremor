APP_NAME=$(shell cat Cargo.toml | grep "name" | sed 's/name = "\(.*\)"/\1/')
TARGET=$(shell rustup default | sed 's/[^-]*-\(.*\) (default)/\1/')

all: build

help: ## Print this help message
	@grep -E '^[a-zA-Z._-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

build: ## Build the executable with Rust's default options
	@cargo build --release
	@echo "Wrote executable to target/release/$(APP_NAME)"

small: ## Build a minimal binary (requirest nightly Rust)
	@cargo +nightly build -Z build-std=std -Z panic-immediate-abort --config 'profile.release.panic="immediate-abort"' --target $(TARGET) --release
	@echo "Wrote executable to target/$(TARGET)/release/$(APP_NAME)"

clean: ## Remove all build artifacts
	@cargo clean

.PHONY: all help build small clean
