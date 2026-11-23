#![allow(non_snake_case)]
//! OpenCanvas 1.1 Wintab shim, v2 ("cursor" mode).
//! The Wacom driver already places the Windows cursor exactly under the pen.
//! This shim replaces the X/Y in every Wintab packet with that cursor position,
//! converted into the coordinate space OpenCanvas asked for. Pressure etc. pass through.
use std::{collections::HashMap, ffi::c_void, fs::OpenOptions, io::Write, sync::{Mutex, OnceLock}};

type H = *mut c_void;
type P = *mut c_void;

#[repr(C)] struct POINT { x: i32, y: i32 }

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryA(n: *const u8) -> H;
    fn GetProcAddress(h: H, n: *const u8) -> *const c_void;
    fn GetSystemDirectoryA(b: *mut u8, n: u32) -> u32;
}
#[link(name = "user32")]
extern "system" { fn GetCursorPos(p: *mut POINT) -> i32; }

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LogContextA {
    lcName: [u8; 40],
    lcOptions: u32, lcStatus: u32, lcLocks: u32, lcMsgBase: u32,
    lcDevice: u32, lcPktRate: u32, lcPktData: u32, lcPktMode: u32,
    lcMoveMask: u32, lcBtnDnMask: u32, lcBtnUpMask: u32,
    lcInOrgX: i32, lcInOrgY: i32, lcInOrgZ: i32,
    lcInExtX: i32, lcInExtY: i32, lcInExtZ: i32,
    lcOutOrgX: i32, lcOutOrgY: i32, lcOutOrgZ: i32,
    lcOutExtX: i32, lcOutExtY: i32, lcOutExtZ: i32,
    lcSensX: u32, lcSensY: u32, lcSensZ: u32,
    lcSysMode: i32,
    lcSysOrgX: i32, lcSysOrgY: i32, lcSysExtX: i32, lcSysExtY: i32,
    lcSysSensX: u32, lcSysSensY: u32,
}

// ---------- config & log ----------
struct Cfg { mode: String, ox: i32, oy: i32, flip_y: bool, verbose: bool }

fn exe_dir() -> std::path::PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_default()
}
fn cfg() -> &'static Cfg {
    static C: OnceLock<Cfg> = OnceLock::new();
    C.get_or_init(|| {
        let mut c = Cfg { mode: "cursor".into(), ox: 0, oy: 0, flip_y: false, verbose: false };
        if let Ok(s) = std::fs::read_to_string(exe_dir().join("wintab_shim.ini")) {
            for line in s.lines() {
                let Some((k, v)) = line.split_once('=') else { continue };
                let (k, v) = (k.trim(), v.trim());
                let b = v == "1" || v.eq_ignore_ascii_case("true");
                match k {
                    "mode" => c.mode = v.to_lowercase(),
                    "offset_x" => c.ox = v.parse().unwrap_or(0),
                    "offset_y" => c.oy = v.parse().unwrap_or(0),
                    "flip_y" => c.flip_y = b,
                    "verbose" => c.verbose = b,
                    _ => {}
                }
            }
        }
        c
    })
}
const MAX_LOG_BYTES: u64 = 256 * 1024; // log is wiped and restarted past 256 KB

fn log(msg: &str) {
    let path = exe_dir().join("wintab_shim.log");
    if std::fs::metadata(&path).map(|m| m.len() > MAX_LOG_BYTES).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{msg}");
    }
}

// ---------- real DLL ----------
fn real_dll() -> H {
    static D: OnceLock<usize> = OnceLock::new();
    *D.get_or_init(|| unsafe {
        let mut buf = [0u8; 260];
        let n = GetSystemDirectoryA(buf.as_mut_ptr(), 260) as usize;
        let mut p = buf[..n].to_vec();
        p.extend_from_slice(b"\\wintab32.dll\0");
        let h = LoadLibraryA(p.as_ptr());
        log(&format!("shim v2 (mode={}) loaded real wintab32: {:?}", cfg().mode, h));
        h as usize
    }) as H
}
fn proc_addr(name: &str) -> usize {
    let mut n = name.as_bytes().to_vec(); n.push(0);
    let p = unsafe { GetProcAddress(real_dll(), n.as_ptr()) } as usize;
    if p == 0 { log(&format!("MISSING in real dll: {name}")); }
    p
}

// ---------- per-context packet layout ----------
#[derive(Clone, Copy)]
struct Ctx { size: usize, xoff: Option<usize>, yoff: Option<usize>,
             org_x: i32, org_y: i32, ext_x: i32, ext_y: i32 }

fn ctxs() -> &'static Mutex<HashMap<usize, Ctx>> {
    static M: OnceLock<Mutex<HashMap<usize, Ctx>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

// Wintab PK_* bits in packet order, with field sizes (32-bit build)
const FIELDS: [(u32, usize); 14] = [
    (0x0001, 4), (0x0002, 4), (0x0004, 4), (0x0008, 4), (0x0010, 4), (0x0020, 4),
    (0x0040, 4), (0x0080, 4), (0x0100, 4), (0x0200, 4), (0x0400, 4), (0x0800, 4),
    (0x1000, 12), (0x2000, 12),
];
const PK_X: u32 = 0x0080;
const PK_Y: u32 = 0x0100;

fn layout(c: &LogContextA) -> Ctx {
    let (mut off, mut xoff, mut yoff) = (0usize, None, None);
    for (bit, sz) in FIELDS {
        if c.lcPktData & bit != 0 {
            if bit == PK_X { xoff = Some(off); }
            if bit == PK_Y { yoff = Some(off); }
            off += sz;
        }
    }
    Ctx { size: off, xoff, yoff, org_x: c.lcOutOrgX, org_y: c.lcOutOrgY,
          ext_x: c.lcOutExtX, ext_y: c.lcOutExtY }
}

/// Overwrite X/Y of `n` packets in `buf` with the current cursor position.
unsafe fn patch(hctx: H, buf: P, n: i32) {
    if buf.is_null() || n <= 0 { return; }
    let cfg = cfg();
    if cfg.mode == "passthrough" { return; }
    let Some(c) = ctxs().lock().ok().and_then(|m| m.get(&(hctx as usize)).copied()) else { return };
    let (Some(xo), Some(yo)) = (c.xoff, c.yoff) else { return };
    let mut pt = POINT { x: 0, y: 0 };
    if GetCursorPos(&mut pt) == 0 { return; }
    // Cursor is in the app's (DPI-virtualized) screen pixels, origin top-left, Y down.
    // OpenCanvas asks for out org=(0,0), ext=(screenW, -screenH): i.e. same space.
    let x = pt.x - c.org_x + cfg.ox;
    let mut y = pt.y - c.org_y + cfg.oy;
    if cfg.flip_y { y = c.ext_y.abs() - 1 - y; }
    for i in 0..n as usize {
        let p = (buf as *mut u8).add(i * c.size);
        if cfg.verbose && i == 0 {
            let ox = (p.add(xo) as *const i32).read_unaligned();
            let oy = (p.add(yo) as *const i32).read_unaligned();
            log(&format!("pkt driver=({ox},{oy}) cursor=({},{}) -> ({x},{y})", pt.x, pt.y));
        }
        (p.add(xo) as *mut i32).write_unaligned(x);
        (p.add(yo) as *mut i32).write_unaligned(y);
    }
}

// ---------- hooked exports ----------
#[no_mangle]
pub unsafe extern "system" fn WTOpenA(hwnd: H, ctx: *mut LogContextA, enable: i32) -> H {
    static F: OnceLock<usize> = OnceLock::new();
    let p = *F.get_or_init(|| proc_addr("WTOpenA"));
    if p == 0 { return std::ptr::null_mut(); }
    let f: unsafe extern "system" fn(H, *mut LogContextA, i32) -> H = std::mem::transmute(p);
    let h = f(hwnd, ctx, enable);   // app's context goes to the driver unchanged
    if !ctx.is_null() && !h.is_null() {
        let c = &*ctx;
        let l = layout(c);
        log(&format!("WTOpenA: pktdata={:#x} pktsize={} x@{:?} y@{:?} out org=({},{}) ext=({},{}) opts={:#x}",
            c.lcPktData, l.size, l.xoff, l.yoff, c.lcOutOrgX, c.lcOutOrgY, c.lcOutExtX, c.lcOutExtY, c.lcOptions));
        if let Ok(mut m) = ctxs().lock() { m.insert(h as usize, l); }
    }
    h
}

#[no_mangle]
pub unsafe extern "system" fn WTSetA(hctx: H, ctx: *mut LogContextA) -> i32 {
    static F: OnceLock<usize> = OnceLock::new();
    let p = *F.get_or_init(|| proc_addr("WTSetA"));
    if p == 0 { return 0; }
    let f: unsafe extern "system" fn(H, *mut LogContextA) -> i32 = std::mem::transmute(p);
    let r = f(hctx, ctx);
    if r != 0 && !ctx.is_null() {
        if let Ok(mut m) = ctxs().lock() { m.insert(hctx as usize, layout(&*ctx)); }
    }
    r
}

#[no_mangle]
pub unsafe extern "system" fn WTClose(hctx: H) -> i32 {
    static F: OnceLock<usize> = OnceLock::new();
    let p = *F.get_or_init(|| proc_addr("WTClose"));
    if let Ok(mut m) = ctxs().lock() { m.remove(&(hctx as usize)); }
    if p == 0 { return 0; }
    let f: unsafe extern "system" fn(H) -> i32 = std::mem::transmute(p);
    f(hctx)
}

#[no_mangle]
pub unsafe extern "system" fn WTPacket(hctx: H, serial: u32, pkt: P) -> i32 {
    static F: OnceLock<usize> = OnceLock::new();
    let p = *F.get_or_init(|| proc_addr("WTPacket"));
    if p == 0 { return 0; }
    let f: unsafe extern "system" fn(H, u32, P) -> i32 = std::mem::transmute(p);
    let r = f(hctx, serial, pkt);
    if r != 0 { patch(hctx, pkt, 1); }
    r
}

#[no_mangle]
pub unsafe extern "system" fn WTPacketsGet(hctx: H, n: i32, buf: P) -> i32 {
    static F: OnceLock<usize> = OnceLock::new();
    let p = *F.get_or_init(|| proc_addr("WTPacketsGet"));
    if p == 0 { return 0; }
    let f: unsafe extern "system" fn(H, i32, P) -> i32 = std::mem::transmute(p);
    let r = f(hctx, n, buf);
    patch(hctx, buf, r);
    r
}

#[no_mangle]
pub unsafe extern "system" fn WTPacketsPeek(hctx: H, n: i32, buf: P) -> i32 {
    static F: OnceLock<usize> = OnceLock::new();
    let p = *F.get_or_init(|| proc_addr("WTPacketsPeek"));
    if p == 0 { return 0; }
    let f: unsafe extern "system" fn(H, i32, P) -> i32 = std::mem::transmute(p);
    let r = f(hctx, n, buf);
    patch(hctx, buf, r);
    r
}

// ---------- plain forwarders ----------
macro_rules! fwd {
    ($name:ident ( $($a:ident : $t:ty),* ) -> $r:ty) => {
        #[no_mangle]
        pub unsafe extern "system" fn $name($($a: $t),*) -> $r {
            static F: OnceLock<usize> = OnceLock::new();
            let p = *F.get_or_init(|| proc_addr(stringify!($name)));
            if p == 0 { return std::mem::zeroed(); }
            let f: unsafe extern "system" fn($($t),*) -> $r = std::mem::transmute(p);
            f($($a),*)
        }
    };
}
fwd!(WTInfoA(cat: u32, idx: u32, out: P) -> u32);
fwd!(WTEnable(h: H, en: i32) -> i32);
fwd!(WTOverlap(h: H, top: i32) -> i32);
fwd!(WTConfig(h: H, wnd: H) -> i32);
fwd!(WTGetA(h: H, ctx: P) -> i32);
fwd!(WTExtGet(h: H, ext: u32, d: P) -> i32);
fwd!(WTExtSet(h: H, ext: u32, d: P) -> i32);
fwd!(WTSave(h: H, d: P) -> i32);
fwd!(WTRestore(wnd: H, d: P, en: i32) -> H);
fwd!(WTDataGet(h: H, b: u32, e: u32, max: i32, buf: P, n: *mut i32) -> i32);
fwd!(WTDataPeek(h: H, b: u32, e: u32, max: i32, buf: P, n: *mut i32) -> i32);
fwd!(WTQueueSizeGet(h: H) -> i32);
fwd!(WTQueueSizeSet(h: H, n: i32) -> i32);
fwd!(WTQueuePacketsEx(h: H, old: *mut u32, new: *mut u32) -> i32);
