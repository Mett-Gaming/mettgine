//! Andere Systeme: keine Ausnahme-Behandlung, keine Meldungsfenster.

pub fn install() {}
pub fn module_base() -> usize {
    0
}
pub fn message_box(_title: &str, _text: &str) {}
