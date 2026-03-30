MyEmulator is now implemented in Rust.

Build:

```bash
make -C runtime/MyEmulator all
```

Run:

```bash
runtime/MyEmulator/build/myemu -i <program.mbin>
```

Notes:
- The instruction encoding remains compatible with the existing toolchain.
- `OUT` to I/O address `0x24000000` prints a byte to host stdout (serial-like console).
