CARGO ?= cargo
BUILD_DIR ?= build
CARGO_TARGET_DIR ?= $(BUILD_DIR)/cargo-target
TARGET ?= $(BUILD_DIR)/myemu

IN ?= tests/outputs/sample.bin
OUT ?=
ARGS ?=

RUST_SOURCES := $(wildcard src/*.rs)

all: $(TARGET)

$(TARGET): Cargo.toml $(RUST_SOURCES)
	mkdir -p $(BUILD_DIR)
	CARGO_TARGET_DIR=$(CARGO_TARGET_DIR) $(CARGO) build --manifest-path Cargo.toml --release
	cp $(CARGO_TARGET_DIR)/release/myemu $(TARGET)
	chmod +x $(TARGET)

run-myemu: $(TARGET)
	@if [ -z "$(OUT)" ]; then \
		./$(TARGET) -i $(IN); \
	else \
		./$(TARGET) -i $(IN) -o $(OUT); \
	fi

gdb: $(TARGET)
	gdb --args ./$(TARGET) $(ARGS)

test:
	CARGO_TARGET_DIR=$(CARGO_TARGET_DIR) $(CARGO) test --manifest-path Cargo.toml

clean:
	rm -rf $(BUILD_DIR)
