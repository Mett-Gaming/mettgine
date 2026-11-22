//! Windows: Abstuerze ausserhalb von Rust (z. B. im Grafiktreiber) abfangen,
//! Hinweisfenster, Adresse des Programms im Speicher.
use std::ffi::c_void;

#[repr(C)]
struct ExceptionRecord {
    code: u32,
    flags: u32,
    next: *mut ExceptionRecord,
    address: *mut c_void,
    params: u32,
    info: [usize; 15],
}

#[repr(C)]
struct ExceptionPointers {
    record: *mut ExceptionRecord,
    context: *mut c_void,
}

type Filter = unsafe extern "system" fn(*mut ExceptionPointers) -> i32;

#[link(name = "kernel32")]
extern "system" {
    fn SetUnhandledExceptionFilter(f: Option<Filter>) -> Option<Filter>;
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(hwnd: *mut c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
}

unsafe extern "system" fn on_exception(info: *mut ExceptionPointers) -> i32 {
    let (code, addr) = if info.is_null() || (*info).record.is_null() {
        (0, 0)
    } else {
        let r = &*(*info).record;
        (r.code, r.address as usize)
    };
    let base = module_base();
    let what = match code {
        0xC000_0005 => "Speicherzugriffsfehler",
        0xC000_00FD => "Stapelueberlauf",
        0xC000_001D => "Ungueltiger Befehl",
        0xC000_0094 => "Division durch null",
        _ => "Absturz",
    };
    let rel = addr
        .checked_sub(base)
        .map(|r| format!(" (Programm+0x{r:x})"))
        .unwrap_or_default();
    let text = format!(
        "{what}: Code 0x{code:08X} an Adresse 0x{addr:x}{rel}\n\
         (ausserhalb von Rust, oft im Grafiktreiber)"
    );
    let path = super::write_report("Absturz (Windows-Ausnahme)", &text);
    super::show_crash_dialog(path.as_deref());
    1 // EXCEPTION_EXECUTE_HANDLER: Prozess beenden
}

pub fn install() {
    unsafe {
        SetUnhandledExceptionFilter(Some(on_exception));
    }
}

pub fn module_base() -> usize {
    unsafe { GetModuleHandleW(std::ptr::null()) as usize }
}

pub fn message_box(title: &str, text: &str) {
    let w = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (t, c) = (w(text), w(title));
    unsafe {
        // MB_OK | MB_ICONERROR | MB_TOPMOST
        MessageBoxW(
            std::ptr::null_mut(),
            t.as_ptr(),
            c.as_ptr(),
            0x10 | 0x0004_0000,
        );
    }
}
