CARGO = cargo
TARGET = wasm32-unknown-unknown

build:
	$(CARGO) build --workspace --release --target $(TARGET)

test:
	$(CARGO) test --workspace

fmt:
	$(CARGO) fmt --all

check:
	$(CARGO) clippy --workspace -- -D warnings

clean:
	cd contracts && $(CARGO) clean
