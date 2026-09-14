# CPU, memory and device contracts

## Ownership

`Machine` connects the CPU, address translation and devices. `cpu::Cpu` owns
registers and execution state; `ram::Ram` owns physical RAM; `DisplayDevice`
owns guest-visible framebuffers and cursor state. `HostDisplay` owns the window,
host input queue and presentation timing. Host display work runs at service
points, never as a side effect of `DISPLAY_SWAP`.

Both normal execution and control-stdio use `run_loop()` and `step()`. A step
delivers a pending interrupt/trap, checks the actual execution PC for a
breakpoint, fetches, executes, then records retirement/profiling. Trace and
profiling PCs therefore refer to the executed instruction, including IRQ entry.
Device poll cadence persists across short control-stdio execution budgets.

## Registers and exceptions

Every guest instruction that writes SR is privileged. Destination validation
precedes operand reads, including consuming MMIO reads and POP. Internal IRQ
entry/IRET use a separate SR restoration path. All SR reads compose the live
condition flags; restoration updates the mode, IE and condition flags together.

CPU virtual memory access returns `Result<_, MmuFault>`; instruction execution
returns `CpuError`, separating guest faults from fatal emulator errors. A
faulting LD/LDB leaves destination registers and flags unchanged. PUSH/POP and
IRET validate their complete RAM access before changing SP. IRQ entry validates
both frame words with kernel permissions before changing SP/KERNEL_SP or RAM.
An unusable interrupt stack is a fatal double-fault diagnostic.

Page/access/privilege faults save the faulting instruction's PC. SYSCALL saves
the next PC. Faults do not count as retired instructions. Execution budgets count
attempts as well as successful instructions, so repeated faults cannot evade a
step limit. A/D page-table metadata can change during a failed access; guest
data memory and architectural instruction results are not partially committed.

Synchronous traps have a pending latch separate from asynchronous IRQ pending
state. They bypass IE, and the latch is consumed on entry. Cause bits remain
visible until W1C acknowledgement but do not themselves repeatedly trigger a
synchronous exception. A synchronous trap without a valid, nonzero, aligned
RAM vector terminates with an error; an asynchronous IRQ waits for a vector.

## Translation and access widths

PDE Valid/User bits gate a subtree. Write/Execute permissions are leaf PTE
permissions; PDE W/X are ignored (preserving the existing MyKernel page-table
format). Effective User permission is PDE.User AND PTE.User. The TLB caches this
effective permission separately from the raw PTE used for A/D updates. Access
decisions must be identical on a page-table walk and a TLB hit.

RAM words are big-endian and may be unaligned. Accesses crossing a virtual page
boundary translate each page. Stores preflight all pages/physical ranges before
writing any data. Address overflow, writes crossing out of RAM into ROM, and
unsupported physical accesses raise an access/page fault with the attempted
read/write/execute status. Instruction fetch and stack words must be aligned.

MMIO word accesses must be aligned, except the legacy UART LSR at `0x24000005`.
Byte accesses are supported only by UART TX/RX/LSR and the automation bridge.
Other MMIO byte accesses and instruction fetches from MMIO fault. A word access
never decomposes into multiple MMIO operations. Physical debugger reads peek;
guest CPU reads can consume RX data.

## SSD DMA

SSD_ADDR is a physical RAM address in both transfer directions. The SSD accepts
only `Ram`, not the CPU bus: DMA cannot invoke MMIO or depend on the active page
directory or CPU mode. The entire 64 KiB range and disk block are validated
before I/O; overflow and RAM-boundary violations report ERROR and completion IRQ
without changing guest data RAM or disk contents.

CMD snapshots command, block and physical address. BLOCK/ADDR writes during BUSY
configure a future command. Additional CMD writes during BUSY are ignored and
leave the original operation and BUSY status intact. Disabled/invalid commands
while idle report ERROR immediately without scheduling a completion IRQ.
Host file I/O occurs synchronously at the next device service point.
Oversized existing disk images are rejected, never truncated to capacity.

## Board display geometry

`config/hardware.conf` is the authoritative board configuration. `build.rs`
validates it and generates Rust dimensions; it does not read MyOS source.
The framebuffer must fit in the 16 MiB VRAM aperture.

| Address | Register | Access |
| --- | --- | --- |
| `0x240000D0` | display width in pixels | read-only word |
| `0x240000D4` | display height in pixels | read-only word |

MyOS reads these registers through `display_config.mln`. MyKernel maps the full
16 MiB aperture, so a resolution change does not require editing OS constants.
MyOS using these registers requires an emulator implementing this interface.

## Verification

`cargo test --locked` covers SR destination checks, cold/warm TLB permissions,
fault repair/retry, syscall return PC, atomic stack frames, cross-page accesses,
MMIO widths, DMA validation/latching, short execution budgets and IRQ debugging.
The workspace QA suites additionally exercise MMU, scheduling, processes,
serial/input and graphics from guest MyLang programs.
