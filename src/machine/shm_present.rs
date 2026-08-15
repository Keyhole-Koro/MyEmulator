// MIT-SHM scan-out.
//
// minifb presents a frame with XPutImage, which pushes the whole 1024x768x4
// framebuffer (3 MB) through the X11 socket every time. Measured on this host
// that costs ~7 ms per frame, which is far more than the guest spends drawing
// the scene (~26 us) and caps the display well under its nominal 60 Hz.
//
// The X server can instead read the pixels straight out of a shared memory
// segment: the frame never travels through the socket, only a small request
// does. We keep minifb for window creation and input and attach this presenter
// to the window it already owns, so the only thing that changes is how pixels
// reach the server. Measured on the same host: ~1.1 ms per frame, or ~2.9 ms
// when forcing a round trip with XSync.
//
// MIT-SHM only works when the server is on this machine. present() reports
// failure rather than panicking so the caller can fall back to minifb.

use std::os::raw::{c_char, c_int, c_uint, c_ulong, c_void};
use x11_dl::xlib;

#[repr(C)]
struct XShmSegmentInfo {
    shmseg: c_ulong,
    shmid: c_int,
    shmaddr: *mut c_char,
    read_only: c_int,
}

type QueryFn = unsafe extern "C" fn(*mut xlib::Display) -> c_int;
type CreateFn = unsafe extern "C" fn(
    *mut xlib::Display,
    *mut xlib::Visual,
    c_uint,
    c_int,
    *mut c_char,
    *mut XShmSegmentInfo,
    c_uint,
    c_uint,
) -> *mut xlib::XImage;
type AttachFn = unsafe extern "C" fn(*mut xlib::Display, *mut XShmSegmentInfo) -> c_int;
type DetachFn = unsafe extern "C" fn(*mut xlib::Display, *mut XShmSegmentInfo) -> c_int;
type PutFn = unsafe extern "C" fn(
    *mut xlib::Display,
    c_ulong,
    xlib::GC,
    *mut xlib::XImage,
    c_int,
    c_int,
    c_int,
    c_int,
    c_uint,
    c_uint,
    c_int,
) -> c_int;

pub struct ShmPresenter {
    xlib: xlib::Xlib,
    display: *mut xlib::Display,
    window: c_ulong,
    gc: xlib::GC,
    image: *mut xlib::XImage,
    info: Box<XShmSegmentInfo>,
    pixels: *mut u32,
    len: usize,
    put: PutFn,
    detach: DetachFn,
}

impl ShmPresenter {
    // `window` is the X11 window id minifb created (Window::get_window_handle).
    // Returns None whenever anything is unavailable, so the caller falls back.
    pub fn new(window_handle: *mut c_void, width: u32, height: u32) -> Option<Self> {
        if window_handle.is_null() {
            return None;
        }
        unsafe {
            let xlib = xlib::Xlib::open().ok()?;

            // The XShm* entry points live in libXext, not libX11.
            let libxext = libc::dlopen(
                b"libXext.so.6\0".as_ptr() as *const c_char,
                libc::RTLD_LAZY,
            );
            if libxext.is_null() {
                return None;
            }
            let sym = |name: &[u8]| -> Option<*mut c_void> {
                let p = libc::dlsym(libxext, name.as_ptr() as *const c_char);
                if p.is_null() { None } else { Some(p) }
            };
            let query: QueryFn = std::mem::transmute(sym(b"XShmQueryExtension\0")?);
            let create: CreateFn = std::mem::transmute(sym(b"XShmCreateImage\0")?);
            let attach: AttachFn = std::mem::transmute(sym(b"XShmAttach\0")?);
            let detach: DetachFn = std::mem::transmute(sym(b"XShmDetach\0")?);
            let put: PutFn = std::mem::transmute(sym(b"XShmPutImage\0")?);

            // Our own connection to the server. minifb owns its display and is
            // not thread-safe to share; a second connection to the same window
            // is fine because the window id is server-side.
            let display = (xlib.XOpenDisplay)(std::ptr::null());
            if display.is_null() {
                return None;
            }
            if query(display) == 0 {
                (xlib.XCloseDisplay)(display);
                return None;
            }

            let screen = (xlib.XDefaultScreen)(display);
            let depth = (xlib.XDefaultDepth)(display, screen);
            let visual = (xlib.XDefaultVisual)(display, screen);
            let window = window_handle as c_ulong;
            let gc = (xlib.XCreateGC)(display, window, 0, std::ptr::null_mut());

            let mut info = Box::new(XShmSegmentInfo {
                shmseg: 0,
                shmid: 0,
                shmaddr: std::ptr::null_mut(),
                read_only: 0,
            });

            const ZPIXMAP: c_int = 2;
            let image = create(
                display,
                visual,
                depth as c_uint,
                ZPIXMAP,
                std::ptr::null_mut(),
                &mut *info,
                width,
                height,
            );
            if image.is_null() {
                (xlib.XFreeGC)(display, gc);
                (xlib.XCloseDisplay)(display);
                return None;
            }

            let size = (*image).bytes_per_line as usize * height as usize;
            let shmid = libc::shmget(libc::IPC_PRIVATE, size, libc::IPC_CREAT | 0o600);
            if shmid < 0 {
                (xlib.XDestroyImage)(image);
                (xlib.XFreeGC)(display, gc);
                (xlib.XCloseDisplay)(display);
                return None;
            }
            let addr = libc::shmat(shmid, std::ptr::null(), 0);
            if addr as isize == -1 {
                libc::shmctl(shmid, libc::IPC_RMID, std::ptr::null_mut());
                (xlib.XDestroyImage)(image);
                (xlib.XFreeGC)(display, gc);
                (xlib.XCloseDisplay)(display);
                return None;
            }

            info.shmid = shmid;
            info.shmaddr = addr as *mut c_char;
            info.read_only = 0;
            (*image).data = addr as *mut c_char;

            if attach(display, &mut *info) == 0 {
                libc::shmdt(addr);
                libc::shmctl(shmid, libc::IPC_RMID, std::ptr::null_mut());
                (xlib.XDestroyImage)(image);
                (xlib.XFreeGC)(display, gc);
                (xlib.XCloseDisplay)(display);
                return None;
            }
            (xlib.XSync)(display, 0);
            // Mark the segment destroyed now; it is freed once both the server
            // and this process detach, so a crash cannot leak it.
            libc::shmctl(shmid, libc::IPC_RMID, std::ptr::null_mut());

            Some(ShmPresenter {
                xlib,
                display,
                window,
                gc,
                image,
                info,
                pixels: addr as *mut u32,
                len: (width * height) as usize,
                put,
                detach,
            })
        }
    }

    // Pointer position relative to the window, and the left-button state,
    // straight from the server. minifb's get_mouse_pos() only returns what its
    // last update() cached, so sampling the pointer through minifb forces the
    // expensive event-queue drain to run at the input cadence. XQueryPointer is
    // a single cheap round trip and needs no event pumping.
    pub fn query_pointer(&mut self) -> Option<(i32, i32, bool)> {
        unsafe {
            let mut root: c_ulong = 0;
            let mut child: c_ulong = 0;
            let mut root_x: c_int = 0;
            let mut root_y: c_int = 0;
            let mut win_x: c_int = 0;
            let mut win_y: c_int = 0;
            let mut mask: c_uint = 0;
            let ok = (self.xlib.XQueryPointer)(
                self.display,
                self.window,
                &mut root,
                &mut child,
                &mut root_x,
                &mut root_y,
                &mut win_x,
                &mut win_y,
                &mut mask,
            );
            if ok == 0 {
                return None; // pointer is on another screen
            }
            const BUTTON1_MASK: c_uint = 1 << 8;
            Some((win_x, win_y, mask & BUTTON1_MASK != 0))
        }
    }

    // Copy one frame into the shared segment and ask the server to blit it.
    pub fn present(&mut self, frame: &[u32], width: u32, height: u32) {
        let n = self.len.min(frame.len());
        unsafe {
            std::ptr::copy_nonoverlapping(frame.as_ptr(), self.pixels, n);
            (self.put)(
                self.display,
                self.window,
                self.gc,
                self.image,
                0,
                0,
                0,
                0,
                width,
                height,
                0,
            );
            // Flush the request without waiting for the server to finish: the
            // blit reads from memory both sides already share.
            (self.xlib.XFlush)(self.display);
        }
    }
}

impl Drop for ShmPresenter {
    fn drop(&mut self) {
        unsafe {
            (self.detach)(self.display, &mut *self.info);
            libc::shmdt(self.info.shmaddr as *const c_void);
            (self.xlib.XDestroyImage)(self.image);
            (self.xlib.XFreeGC)(self.display, self.gc);
            (self.xlib.XCloseDisplay)(self.display);
        }
    }
}
