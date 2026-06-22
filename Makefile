CARGO ?= cargo
BUILD_DIR ?= build
CARGO_TARGET_DIR ?= $(BUILD_DIR)/cargo-target
TARGET ?= $(BUILD_DIR)/myemu

IN ?= tests/outputs/sample.bin
OUT ?=
ARGS ?=
BREAK ?=
STEP ?=
MEM_ADDR ?=
MEM_LEN ?=

RUST_SOURCES := $(shell find src -name '*.rs')

all: $(TARGET)

$(TARGET): Cargo.toml $(RUST_SOURCES)
	mkdir -p $(BUILD_DIR)
	CARGO_TARGET_DIR=$(CARGO_TARGET_DIR) $(CARGO) build --manifest-path Cargo.toml --release
	cp $(CARGO_TARGET_DIR)/release/myemu $(TARGET).tmp
	mv -f $(TARGET).tmp $(TARGET)
	chmod +x $(TARGET)

run-myemu: $(TARGET)
	@if [ -z "$(OUT)" ]; then \
		./$(TARGET) -i $(IN); \
	else \
		./$(TARGET) -i $(IN) -o $(OUT); \
	fi

debug-myemu: $(TARGET)
	@if [ -z "$(OUT)" ]; then \
		./$(TARGET) -i $(IN) $(ARGS); \
	else \
		./$(TARGET) -i $(IN) -o $(OUT) $(ARGS); \
	fi

trace-myemu: $(TARGET)
	./$(TARGET) -i $(IN) --trace $(ARGS)

break-myemu: $(TARGET)
	@if [ -z "$(BREAK)" ]; then \
		echo "Usage: make -C runtime/MyEmulator break-myemu IN=<program.mbin> BREAK=<addr> [ARGS='--regs']"; \
		exit 1; \
	fi
	./$(TARGET) -i $(IN) --break $(BREAK) $(ARGS)

step-myemu: $(TARGET)
	@if [ -z "$(STEP)" ]; then \
		echo "Usage: make -C runtime/MyEmulator step-myemu IN=<program.mbin> STEP=<n> [ARGS='--regs']"; \
		exit 1; \
	fi
	./$(TARGET) -i $(IN) --step $(STEP) $(ARGS)

mem-myemu: $(TARGET)
	@if [ -z "$(MEM_ADDR)" ] || [ -z "$(MEM_LEN)" ]; then \
		echo "Usage: make -C runtime/MyEmulator mem-myemu IN=<program.mbin> MEM_ADDR=<addr> MEM_LEN=<len>"; \
		exit 1; \
	fi
	./$(TARGET) -i $(IN) --mem $(MEM_ADDR) $(MEM_LEN) $(ARGS)

gdb: $(TARGET)
	gdb --args ./$(TARGET) $(ARGS)

test:
	CARGO_TARGET_DIR=$(CARGO_TARGET_DIR) $(CARGO) test --manifest-path Cargo.toml

clean:
	rm -rf $(BUILD_DIR)
