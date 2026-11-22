use super::layout;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

type Hwnd = *mut c_void;
type WndProc = unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize;

#[repr(C)]
struct WndClassExW {
    size: u32,
    style: u32,
    proc_: Option<WndProc>,
    cls_extra: i32,
    wnd_extra: i32,
    instance: *mut c_void,
    icon: *mut c_void,
    cursor: *mut c_void,
    background: *mut c_void,
    menu_name: *const u16,
    class_name: *const u16,
    icon_small: *mut c_void,
}

#[repr(C)]
struct Msg {
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    pt: [i32; 2],
    private: u32,
}

#[repr(C)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[link(name = "user32")]
extern "system" {
    fn RegisterClassExW(c: *const WndClassExW) -> u16;
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: Hwnd,
        menu: *mut c_void,
        inst: *mut c_void,
        param: *mut c_void,
    ) -> Hwnd;
    fn DefWindowProcW(h: Hwnd, m: u32, w: usize, l: isize) -> isize;
    fn GetMessageW(m: *mut Msg, h: Hwnd, a: u32, b: u32) -> i32;
    fn TranslateMessage(m: *const Msg) -> i32;
    fn DispatchMessageW(m: *const Msg) -> isize;
    fn IsDialogMessageW(h: Hwnd, m: *mut Msg) -> i32;
    fn PostQuitMessage(code: i32);
    fn SendMessageW(h: Hwnd, m: u32, w: usize, l: isize) -> isize;
    fn MoveWindow(h: Hwnd, x: i32, y: i32, w: i32, hgt: i32, repaint: i32) -> i32;
    fn GetClientRect(h: Hwnd, r: *mut Rect) -> i32;
    fn GetDlgItem(h: Hwnd, id: i32) -> Hwnd;
    fn LoadCursorW(inst: *mut c_void, name: *const u16) -> *mut c_void;
    fn GetSystemMetrics(i: i32) -> i32;
    fn GetDpiForSystem() -> u32;
    fn SetProcessDPIAware() -> i32;
    fn DestroyWindow(h: Hwnd) -> i32;
    fn SetWindowTextW(h: Hwnd, s: *const u16) -> i32;
    fn OpenClipboard(h: Hwnd) -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(fmt: u32, mem: *mut c_void) -> *mut c_void;
    fn CloseClipboard() -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GlobalAlloc(flags: u32, bytes: usize) -> *mut c_void;
    fn GlobalLock(mem: *mut c_void) -> *mut c_void;
    fn GlobalUnlock(mem: *mut c_void) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    fn CreateFontW(
        h: i32,
        w: i32,
        esc: i32,
        orient: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strike: u32,
        charset: u32,
        out: u32,
        clip: u32,
        quality: u32,
        pitch: u32,
        face: *const u16,
    ) -> *mut c_void;
}

const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_CHILD: u32 = 0x4000_0000;
const WS_TABSTOP: u32 = 0x0001_0000;
const WS_VSCROLL: u32 = 0x0020_0000;
const WS_HSCROLL: u32 = 0x0010_0000;
const WS_BORDER: u32 = 0x0080_0000;
const ES_MULTILINE: u32 = 0x0004;
const ES_READONLY: u32 = 0x0800;
const ES_AUTOVSCROLL: u32 = 0x0040;
const ES_AUTOHSCROLL: u32 = 0x0080;
const BS_DEFPUSHBUTTON: u32 = 0x0001;
const WM_DESTROY: u32 = 0x0002;
const WM_SIZE: u32 = 0x0005;
const WM_CLOSE: u32 = 0x0010;
const WM_SETFONT: u32 = 0x0030;
const WM_COMMAND: u32 = 0x0111;
const WM_GETMINMAXINFO: u32 = 0x0024;
const COLOR_BTNFACE: usize = 15;
const IDC_ARROW: usize = 32512;

const ID_TITLE: i32 = 100;
const ID_TEXT: i32 = 101;
const ID_REPORT: i32 = 102;
const ID_COPY: i32 = 200;
const ID_FOLDER: i32 = 201;
const ID_RESTART: i32 = 202;
const ID_CLOSE: i32 = 203;
const IDS: [i32; 7] = [
    ID_TITLE, ID_TEXT, ID_REPORT, ID_COPY, ID_FOLDER, ID_RESTART, ID_CLOSE,
];

struct Shared {
    report_text: String,
    report_path: PathBuf,
    scale: f32,
}
static SHARED: OnceLock<Shared> = OnceLock::new();

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Windows-Zeilenenden fuer das Textfeld
fn crlf(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\n', "\r\n")
}

unsafe fn relayout(h: Hwnd) {
    let mut r = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    GetClientRect(h, &mut r);
    let s = SHARED.get().map_or(1.0, |x| x.scale);
    for (id, (x, y, cw, ch)) in IDS.iter().zip(layout(r.right, r.bottom, s)) {
        MoveWindow(GetDlgItem(h, *id), x, y, cw, ch, 1);
    }
}

fn copy_to_clipboard(h: Hwnd, text: &str) -> bool {
    let data: Vec<u16> = crlf(text).encode_utf16().chain(Some(0)).collect();
    unsafe {
        if OpenClipboard(h) == 0 {
            return false;
        }
        EmptyClipboard();
        let mem = GlobalAlloc(0x0002, data.len() * 2); // GMEM_MOVEABLE
        if mem.is_null() {
            CloseClipboard();
            return false;
        }
        let p = GlobalLock(mem) as *mut u16;
        std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
        GlobalUnlock(mem);
        SetClipboardData(13, mem); // CF_UNICODETEXT
        CloseClipboard();
    }
    true
}

fn open_folder(path: &Path) {
    let _ = std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn();
}

fn restart() {
    if let Ok(exe) = std::env::current_exe() {
        let mut cmd = std::process::Command::new(&exe);
        if let Some(dir) = exe.parent() {
            cmd.current_dir(dir);
        }
        let _ = cmd.spawn();
    }
}

unsafe extern "system" fn proc_(h: Hwnd, msg: u32, wp: usize, lp: isize) -> isize {
    match msg {
        WM_SIZE => {
            relayout(h);
            0
        }
        WM_GETMINMAXINFO => {
            // kleinste Fenstergroesse
            let s = SHARED.get().map_or(1.0, |x| x.scale);
            let info = lp as *mut [i32; 10];
            if !info.is_null() {
                (*info)[6] = (460.0 * s) as i32;
                (*info)[7] = (300.0 * s) as i32;
            }
            0
        }
        WM_COMMAND => {
            let id = (wp & 0xFFFF) as i32;
            let Some(sh) = SHARED.get() else { return 0 };
            match id {
                ID_COPY => {
                    let ok = copy_to_clipboard(h, &sh.report_text);
                    let label = if ok {
                        "Kopiert!"
                    } else {
                        "Kopieren ging nicht"
                    };
                    SetWindowTextW(GetDlgItem(h, ID_COPY), w(label).as_ptr());
                }
                ID_FOLDER => open_folder(&sh.report_path),
                ID_RESTART => {
                    restart();
                    DestroyWindow(h);
                }
                ID_CLOSE => {
                    DestroyWindow(h);
                }
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(h);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

pub fn show(title: &str, headline: &str, text: &str, report: &str, path: &Path) {
    unsafe {
        SetProcessDPIAware();
        let scale = GetDpiForSystem().max(96) as f32 / 96.0;
        let _ = SHARED.set(Shared {
            report_text: report.to_string(),
            report_path: path.to_path_buf(),
            scale,
        });
        let inst = GetModuleHandleW(std::ptr::null());
        let class = w("MettgineCrashWindow");
        let wc = WndClassExW {
            size: std::mem::size_of::<WndClassExW>() as u32,
            style: 0,
            proc_: Some(proc_),
            cls_extra: 0,
            wnd_extra: 0,
            instance: inst,
            icon: std::ptr::null_mut(),
            cursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW as *const u16),
            background: (COLOR_BTNFACE + 1) as *mut c_void,
            menu_name: std::ptr::null(),
            class_name: class.as_ptr(),
            icon_small: std::ptr::null_mut(),
        };
        RegisterClassExW(&wc);
        let (ww, wh) = ((760.0 * scale) as i32, (560.0 * scale) as i32);
        let x = (GetSystemMetrics(0) - ww) / 2;
        let y = (GetSystemMetrics(1) - wh) / 2;
        let caption = w(&format!("{title}: Absturz"));
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            caption.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            x,
            y,
            ww,
            wh,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            inst,
            std::ptr::null_mut(),
        );
        if hwnd.is_null() {
            return;
        }
        let face = w("Segoe UI");
        let mono = w("Consolas");
        let font = |px: f32, weight: i32, f: &[u16]| {
            CreateFontW(
                -((px * scale) as i32),
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                1,
                0,
                0,
                5,
                0,
                f.as_ptr(),
            )
        };
        let f_title = font(22.0, 700, &face);
        let f_text = font(15.0, 400, &face);
        let f_mono = font(13.0, 400, &mono);
        let child = |cls: &str, txt: &str, style: u32, id: i32, f: *mut c_void| {
            let hc = CreateWindowExW(
                0,
                w(cls).as_ptr(),
                w(txt).as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                0,
                0,
                10,
                10,
                hwnd,
                id as usize as *mut c_void,
                inst,
                std::ptr::null_mut(),
            );
            SendMessageW(hc, WM_SETFONT, f as usize, 1);
            hc
        };
        child("STATIC", headline, 0, ID_TITLE, f_title);
        child("STATIC", &crlf(text), 0, ID_TEXT, f_text);
        child(
            "EDIT",
            &crlf(report),
            WS_BORDER
                | WS_VSCROLL
                | WS_HSCROLL
                | ES_MULTILINE
                | ES_READONLY
                | ES_AUTOVSCROLL
                | ES_AUTOHSCROLL
                | WS_TABSTOP,
            ID_REPORT,
            f_mono,
        );
        child(
            "BUTTON",
            "Bericht kopieren",
            WS_TABSTOP | BS_DEFPUSHBUTTON,
            ID_COPY,
            f_text,
        );
        child("BUTTON", "Ordner öffnen", WS_TABSTOP, ID_FOLDER, f_text);
        child("BUTTON", "Neu starten", WS_TABSTOP, ID_RESTART, f_text);
        child("BUTTON", "Schließen", WS_TABSTOP, ID_CLOSE, f_text);
        relayout(hwnd);
        let mut msg: Msg = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &mut msg) != 0 {
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
