//! Uebersetzungen: je Sprache eine Textdatei `schluessel = Text`, Platzhalter `{name}`.
//!
//! ```
//! use mettgine::locale::Locale;
//! let mut l = Locale::new("de", "punkte = {n} Punkte\nhallo = Hallo");
//! l.add("en", "punkte = {n} points");
//! l.set("en");
//! assert_eq!(l.with("punkte", &[("n", "3")]), "3 points");
//! assert_eq!(l.text("hallo"), "Hallo"); // fehlt auf Englisch: Vorgabe
//! ```

use std::collections::HashMap;

type Table = HashMap<String, String>;

/// Texte eines Spiels in mehreren Sprachen.
#[derive(Clone, Debug)]
pub struct Locale {
    languages: Vec<(String, Table)>,
    current: usize,
}

/// Zeilen `schluessel = Text`; `#` leitet Kommentare ein, `\n` im Text ist ein Umbruch.
fn parse(text: &str) -> Table {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().replace("\\n", "\n")))
        .collect()
}

impl Locale {
    /// Vorgabe-Sprache: hat alle Texte und springt ein, wenn einer fehlt.
    pub fn new(language: &str, text: &str) -> Self {
        Self {
            languages: vec![(language.to_string(), parse(text))],
            current: 0,
        }
    }

    /// Weitere Sprache (ersetzt eine gleichnamige).
    pub fn add(&mut self, language: &str, text: &str) {
        let table = parse(text);
        match self.languages.iter_mut().find(|l| l.0 == language) {
            Some(l) => l.1 = table,
            None => self.languages.push((language.to_string(), table)),
        }
    }

    /// Sprache waehlen; `false` = gibt es nicht. `de-AT` faellt auf `de` zurueck.
    pub fn set(&mut self, language: &str) -> bool {
        let short = language.split(['-', '_']).next().unwrap_or(language);
        let find = |n: &str| self.languages.iter().position(|l| l.0.eq_ignore_ascii_case(n));
        match find(language).or_else(|| find(short)) {
            Some(i) => {
                self.current = i;
                true
            }
            None => false,
        }
    }

    pub fn current(&self) -> &str {
        &self.languages[self.current].0
    }

    pub fn languages(&self) -> impl Iterator<Item = &str> {
        self.languages.iter().map(|l| l.0.as_str())
    }

    /// Text zu einem Schluessel; fehlt er ueberall, der Schluessel selbst.
    pub fn text<'a>(&'a self, key: &'a str) -> &'a str {
        let of = |i: usize| self.languages[i].1.get(key).map(String::as_str);
        of(self.current).or_else(|| of(0)).unwrap_or(key)
    }

    /// Text mit eingesetzten Platzhaltern.
    pub fn with(&self, key: &str, values: &[(&str, &str)]) -> String {
        let text = self.text(key);
        let mut out = String::with_capacity(text.len() + 16);
        let mut rest = text;
        while let Some(i) = rest.find('{') {
            out.push_str(&rest[..i]);
            rest = &rest[i..];
            let Some(end) = rest.find('}') else {
                break;
            };
            match values.iter().find(|v| v.0 == &rest[1..end]) {
                Some((_, v)) => out.push_str(v),
                None => out.push_str(&rest[..=end]),
            }
            rest = &rest[end + 1..];
        }
        out.push_str(rest);
        out
    }

    /// Schluessel, die in `language` fehlen (fuer Tests des Spiels).
    pub fn missing(&self, language: &str) -> Vec<&str> {
        let Some(l) = self.languages.iter().find(|l| l.0 == language) else {
            return Vec::new();
        };
        let base = &self.languages[0].1;
        let mut m: Vec<&str> = base
            .keys()
            .filter(|k| !l.1.contains_key(*k))
            .map(String::as_str)
            .collect();
        m.sort_unstable();
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_fall_back_and_placeholders_are_filled() {
        let mut l = Locale::new("de", "# Menue\nstart = Los\\ngeht's\nrest = {a} von {b}, {c}\n");
        l.add("en", "start = Go");
        assert!(l.set("en-US") && l.current() == "en");
        assert!(!l.set("fr") && l.current() == "en");
        assert_eq!(l.text("start"), "Go");
        assert_eq!(l.with("rest", &[("a", "1"), ("b", "{a}")]), "1 von {a}, {c}");
        assert_eq!(l.text("fehlt"), "fehlt");
        assert_eq!(l.missing("en"), ["rest"]);
        assert!(l.set("de"));
        assert_eq!(l.text("start"), "Los\ngeht's");
        assert_eq!(l.languages().collect::<Vec<_>>(), ["de", "en"]);
    }
}
