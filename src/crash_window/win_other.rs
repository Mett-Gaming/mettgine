//! Andere Systeme: Bericht auf die Konsole.

pub fn show(_t: &str, headline: &str, text: &str, report: &str, _p: &std::path::Path) {
    eprintln!("{headline}\n{text}\n\n{report}");
}
