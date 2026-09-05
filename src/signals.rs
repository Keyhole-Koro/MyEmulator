use std::sync::atomic::{AtomicBool, Ordering};

// Set by the SIGINT/SIGTERM handler. The run loop polls this and exits cleanly
// when set, so the Machine (and its minifb Window) is dropped normally, which
// destroys the X11 window instead of leaving it orphaned.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_signal(_sig: libc::c_int) {
    // Only async-signal-safe work here: flip an atomic flag.
    SHUTDOWN.store(true, Ordering::SeqCst);
}

// Install handlers for SIGINT (Ctrl+C) and SIGTERM (kill / harness shutdown).
// Safe to call once at startup.
pub fn install() {
    let handler = handle_signal as *const () as libc::sighandler_t;
    unsafe {
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
}

// True once a shutdown signal has been received.
pub fn requested() -> bool {
    SHUTDOWN.load(Ordering::SeqCst)
}
