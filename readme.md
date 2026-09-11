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
python3 qa/runners/run_mylang.py toolchain/MyLangCompiler/tests/succeed/function/simpleFunc.mln --reg R1
```

Debug session logs:

```bash
python3 qa/runners/run_mylang.py toolchain/MyLangCompiler/tests/succeed/function/simpleFunc.mln --trace --mem 0x0 64
python3 qa/runners/run_kernel.py --trace
```

Each run writes a session directory with:
- `manifest.json` for commands, exit codes, sources, and artifact paths.
- `combined.log` plus per-step logs under `steps/`.
- generated binaries and intermediate files under `artifacts/`.
- emulator outputs such as `registers.txt`, `serial.txt`, `trace.log`, and `memory/final-00000000-0000ffff.txt` when the emulator binary supports `--log-dir`.

Run the bundled MyLang serial-debug sample:

```bash
python3 qa/runners/run_mylang.py runtime/MyEmulator/examples/mylang_debug --masm --reg R1
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

## Display, input and screenshots (MYOS-014)

The register map lives in `src/constants.rs`; the guest mirrors it in
`system/MyOS/src/ui/graphics.mln`, `system/MyKernel/src/io/mouse.mln` and
`system/MyKernel/src/io/keyboard.mln`.

### 2D accelerator (DMA2D)

`DEST/WIDTH/HEIGHT/STRIDE` describe the destination rectangle in VRAM,
`COLOR` is `0x00RRGGBB` (opaque) or `0xAARRGGBB` where alpha matters, and
writing `CMD` runs the operation:

| CMD | Name | Extra registers | Effect |
|---|---|---|---|
| 1 | FILL | – | `dest = COLOR` |
| 2 | BLEND_FILL | – | `dest = lerp(dest, COLOR, COLOR.a)` |
| 3 | COPY | `SRC`, `SRC_STRIDE` (px) | copy pixels from RAM or VRAM (overlap-safe) |
| 4 | COPY_BLEND | `SRC`, `SRC_STRIDE` (px) | per-pixel alpha from the source |
| 5 | MASK_A8 | `SRC`, `SRC_STRIDE` (bytes) | 8-bit coverage × `COLOR` (anti-aliased glyphs) |
| 6 / 7 | GRADIENT_V / GRADIENT_H | `COLOR2` | linear gradient `COLOR → COLOR2` |
| 8 | ROUND_RECT | `RADIUS` | anti-aliased rounded rectangle |
| 9 | ROUND_RECT_OUTLINE | `RADIUS`, `SPREAD` (thickness) | ring inside the rectangle |
| 10 | SHADOW | `RADIUS`, `SPREAD` (blur) | soft drop shadow around the rectangle |

`CLIP_X0/Y0/X1/Y1` is a scissor applied to every command (disabled while
`X1 <= X0`), so the guest can repaint a damaged region without reshaping
what it draws. The datapath is `src/machine/dma2d.rs`.

### Keyboard and mouse

Key presses, releases and host-translated characters land in one FIFO
(`KBD_EVT_*`, `IRQ_CAUSE_KEYBOARD`) in the order they happened: a shifted
keystroke arrives as `DOWN 'a'`, `CHAR 'A'`, `UP 'a'`. Printable keys use
their lowercase ASCII as the code, everything else a `KEY_*` value `>= 0x100`.
The mouse reports left/right/middle buttons (`MOUSE_BUTTON_*`) and wheel
steps per event (`MOUSE_EVT_WHEEL`).

### Screenshots

```bash
myemu -i build/firmware_linked.mbin --disk build/disk.img --headless --step 200000000 --screenshot shot.png
make screenshot            # the same via qa/runners/run_system.py
```

PNG or PPM by extension; `--step` stops at the first idle (WFI), i.e. right
after the desktop painted. In `--control-stdio` mode the `screenshot`
command accepts either format too.

### Control-stdio commands

Besides `mouse.move/down/up`, `frame.wait`, `dom.snapshot` and `screenshot`
(see `src/control_stdio.rs`): `mouse.down`/`mouse.up` take `"button":"right"`,
`mouse.wheel` takes `"steps"`, `key.type` feeds `"text"` as CHAR events, and
`key.press`/`key.release` take a `"key"` name (`enter`, `backspace`, `left`,
…) or a single character. `frame.wait` now runs the guest until it presents
a frame, so a following screenshot shows the reaction to the input.

### Debugging preemption

`MYEMU_IRQ_CHECK=1` snapshots the CPU state at every IRQ entry and reports,
at the matching `iret`, any register or condition flag the handler failed to
restore. It found the stale-SR-flags bug fixed in `registers.rs`
(`status_register()`), which corrupted any computation interrupted between a
compare and its branch.
