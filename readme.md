MyEmulator is now implemented in Rust.

Build:

```bash
make -C runtime/MyEmulator all
```

Run:

```bash
make -C runtime/MyEmulator run-myemu IN=<program.mbin>
make -C runtime/MyEmulator debug-myemu IN=<program.mbin> ARGS="--regs"
```

Debugger-style options:

```bash
make -C runtime/MyEmulator trace-myemu IN=<program.mbin>
make -C runtime/MyEmulator break-myemu IN=<program.mbin> BREAK=0x40 ARGS="--regs"
make -C runtime/MyEmulator step-myemu IN=<program.mbin> STEP=10 ARGS="--regs"
make -C runtime/MyEmulator mem-myemu IN=<program.mbin> MEM_ADDR=0x00000000 MEM_LEN=64
```

Run a MyLang program end-to-end:

```bash
python3 qa/run_mylang.py toolchain/MyLangCompiler/tests/succeed/function/simpleFunc.mln --reg R1
```

Run the bundled MyLang serial-debug sample:

```bash
python3 qa/run_mylang.py runtime/MyEmulator/examples/mylang_debug --masm --reg R1
```

Useful debug notes:
- The linked `.mbin`, generated `.masm`, and `.mobj` files are kept under `qa/outputs/run_mylang/<name>/` by default.
- `--entry <name>` lets you pick a non-`main` function as the emulator entry point.
- `--no-run` builds only, which is handy when you want to inspect generated assembly before executing it.
- `runtime/MyEmulator/examples/mylang_debug/` contains a tiny debug runtime:
  `debug_putc`, `debug_puts`, and `debug_print_hex_u32`.

Notes:
- The instruction encoding remains compatible with the existing toolchain.
- `OUT` to I/O address `0x24000000` prints a byte to host stdout (serial-like console).
- `--break` and `--mem` accept decimal or hex addresses like `64` or `0x40`.
