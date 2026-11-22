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
- emulator outputs such as `registers.txt`, `serial.txt`, `trace.log`, and `memory/final-00000000-0000ffff.txt`: the runners pass the session directory to the emulator as `--log-dir`.

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

### Board and execution contracts

Display geometry is configured in `config/hardware.conf` and exposed to MyOS
through read-only MMIO at `0x240000D0` (width) / `0x240000D4` (height).
The emulator builds independently of the MyOS source tree.
See [CPU, memory and device contracts](docs/execution-contracts.md) for exception
semantics, access widths, DMA ownership and the shared execution loop.

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

`CLIP_X0/Y0/X1/Y1` is a scissor applied to every command, so the guest can
repaint a damaged region without reshaping what it draws. It is disabled
while `X1 <= X0` (the reset state); enabled with `Y1 <= Y0`, it covers no
pixel and commands write nothing. An empty clip therefore cannot be handed
to the device: a guest whose clip is empty must not submit commands at all.
The datapath is `src/machine/dma2d.rs`.

A command runs in the emulator the moment the guest writes `CMD`, so its
cost is wall time that no guest instruction count shows. ROUND_RECT, its
outline and SHADOW work out anti-aliased coverage per pixel only near the
edges and corners: deeper inside, the coverage is exactly full, and the
solid middle is filled (or, for an outline, left alone) without the
distance formula -- the same pixels, a few times faster on a window-sized
rectangle (`solid_span`; a test compares it with the per-pixel loops).

### Keyboard and mouse

Key presses, releases and host-translated characters land in one FIFO
(`KBD_EVT_*`, `IRQ_CAUSE_KEYBOARD`) in the order they happened: a shifted
keystroke arrives as `DOWN 'a'`, `CHAR 'A'`, `UP 'a'`. Printable keys use
their lowercase ASCII as the code, everything else a `KEY_*` value `>= 0x100`.
The mouse reports left/right/middle buttons (`MOUSE_BUTTON_*`) and wheel
steps per event (`MOUSE_EVT_WHEEL`).

### Presenting frames

The window gets a frame when the guest swaps (`DISPLAY_SWAP`) or the cursor
moves, not at every refresh: an unchanged screen costs nothing but a
keepalive twice a second. With MIT-SHM the hardware cursor is drawn straight
into the shared segment; when only the cursor moved, just the pixels under
its old and new sprite are restored and sent. A put is asynchronous -- the
segment is not written again until the server's completion event arrives --
so the CPU thread never waits on the X server (`src/machine/host_display.rs`,
`src/machine/shm_present.rs`). `--io-stats` counts the presents by kind and
reports the guest's measured speed.

### Frame timeline

`--timeline out.json` records, on the wall clock, what the guest and the
emulator did, and writes a Chrome trace at exit (open it at
https://ui.perfetto.dev; `python3 tools/MyProfiler/myprofiler.py frames
out.json` summarizes it). Its tracks:

- **guest CPU**: spans the guest marks through the timeline registers
  (`system/MyKernel/src/debug/timeline.mln`: MyOS marks each compositor pass,
  its input, requests and paint, and each window's paint under its title),
  device interrupts, and the guest's idle time (WFI); a counter of its speed.
- **emulator (host)**: presenting a frame and pumping the window's events.
- **screen**: guest swaps and cursor moves.
- **2D DMA (host)**: each DMA command over 4096 pixels or 200 µs, with its size.

Every event carries the instructions retired so far. In control-stdio mode,
`timeline.start` begins a recording and `timeline.write {"path"}` writes and
ends it. `make run ARGS="--timeline build/timeline.json"` records a session.

### Screenshots

```bash
myemu -i build/firmware_linked.mbin --disk build/disk.img --headless --step 200000000 --screenshot shot.png
make screenshot            # the same via qa/runners/run_system.py
```

PNG or PPM by extension; `--step` stops at the first idle (WFI), i.e. right
after the desktop painted. In `--control-stdio` mode the `screenshot`
command accepts either format too.

### Control-stdio protocol

`--control-stdio` is protocol v2: stdin/stdout carry only request-ID JSON
Lines, while guest serial output and the emulator's own messages (the DEBUG
instruction's registers and memory dump, `--verbose` and `--trace` logs) go
to stderr. Start with `session.hello`,
then use `os.ready`, `dom.snapshot`, `input.pointer.sequence`,
`input.key.type`, `input.key.press`, `screen.screenshot`, `screen.cursor`
(where the hardware cursor is) and `timeline.start` / `timeline.write` (see
`src/control_stdio.rs`). DOM snapshots cross a dedicated automation MMIO
bridge; they do not type a command into the OS shell or parse serial markers.

With `--profile` (and `--profile-thread-var`, so it follows threads) a client
can also record what ran, in order. `trace.start` takes `watch`, a list of
function entry PCs, each a number or `{"pc", "space"}` for one address space,
and optionally `"all": true`, `"interrupts": false` and a `limit`. From then on
the log holds every call to a watched entry and its return, every interrupt
with its cause bits (for a syscall also r1 and r5), the `iret` back, each
thread switch and each control request. Every event has the instructions
retired so far and the timer's clock. `trace.read {"since": n}` returns the
log from event n, `trace.write {"path"}` writes it to a file and `trace.stop`
ends it. A call is a `call`, or a `mov pc, rX` from any register but LR,
which is how a function value is called; the profile counts both. See
`src/machine/trace.rs`.

### Debugging preemption

`MYEMU_IRQ_CHECK=1` snapshots the CPU state at every IRQ entry and reports,
at the matching `iret`, any register or condition flag the handler failed to
restore. It found the stale-SR-flags bug fixed in `registers.rs`
(`status_register()`), which corrupted any computation interrupted between a
compare and its branch.
