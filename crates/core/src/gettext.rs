// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The translations of `po/` for front ends without `KI18n`: a reader for gettext catalogues
//! with contexts, and the little of KDE's markup that the texts use.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The translations of one language.
#[derive(Debug, Default)]
pub struct Catalog {
    messages: HashMap<(String, String), String>,
}

impl Catalog {
    /// Reads a `.po` file. Fuzzy and untranslated entries do not count, as with `msgfmt`.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut messages = HashMap::new();
        let mut entry = Entry::default();
        let mut field = Field::None;
        for line in text.lines().map(str::trim) {
            if line.is_empty() {
                entry.finish(&mut messages);
                field = Field::None;
            } else if let Some(flags) = line.strip_prefix("#,") {
                entry.fuzzy |= flags.split(',').any(|flag| flag.trim() == "fuzzy");
            } else if line.starts_with('#') {
                // A comment.
            } else if let Some(rest) = line.strip_prefix("msgctxt ") {
                if entry.text.is_some() {
                    entry.finish(&mut messages);
                }
                field = Field::Context;
                entry.context = Some(unquote(rest));
            } else if let Some(rest) = line.strip_prefix("msgid ") {
                if entry.text.is_some() {
                    entry.finish(&mut messages);
                }
                field = Field::Text;
                entry.text = Some(unquote(rest));
            } else if let Some(rest) = line.strip_prefix("msgstr ") {
                field = Field::Translation;
                entry.translation = Some(unquote(rest));
            } else if line.starts_with('"') {
                let more = unquote(line);
                let target = match field {
                    Field::Context => entry.context.as_mut(),
                    Field::Text => entry.text.as_mut(),
                    Field::Translation => entry.translation.as_mut(),
                    Field::None => None,
                };
                if let Some(target) = target {
                    target.push_str(&more);
                }
            }
        }
        entry.finish(&mut messages);
        Self { messages }
    }

    /// The translation of `text` in `context`, if there is one.
    #[must_use]
    pub fn get(&self, context: &str, text: &str) -> Option<&str> {
        self.messages
            .get(&(context.to_string(), text.to_string()))
            .map(String::as_str)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

#[derive(Clone, Copy)]
enum Field {
    None,
    Context,
    Text,
    Translation,
}

#[derive(Default)]
struct Entry {
    context: Option<String>,
    text: Option<String>,
    translation: Option<String>,
    fuzzy: bool,
}

impl Entry {
    fn finish(&mut self, messages: &mut HashMap<(String, String), String>) {
        let entry = std::mem::take(self);
        if let (Some(text), Some(translation)) = (entry.text, entry.translation)
            && !text.is_empty()
            && !translation.is_empty()
            && !entry.fuzzy
        {
            messages.insert((entry.context.unwrap_or_default(), text), translation);
        }
    }
}

/// The text of a quoted string with the escapes of C.
fn unquote(text: &str) -> String {
    let inner = text
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(text);
    let mut result = String::with_capacity(inner.len());
    let mut characters = inner.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            result.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => result.push('\n'),
            Some('t') => result.push('\t'),
            Some('r') => result.push('\r'),
            Some(other) => result.push(other),
            None => result.push('\\'),
        }
    }
    result
}

/// The catalogue among `catalogues`, named like `de` or `pt_BR`, for the first of
/// `languages` that has one, in the form Qt gives them, such as `de-DE` or `zh-Hant-TW`.
#[must_use]
pub fn choose<'a>(languages: &[String], catalogues: &[(&str, &'a str)]) -> Option<&'a str> {
    let find = |name: &str| {
        catalogues
            .iter()
            .find(|(found, _)| *found == name)
            .map(|(_, text)| *text)
    };
    languages.iter().find_map(|language| {
        let name = language.replace('-', "_");
        let base = name.split('_').next().unwrap_or_default();
        let script = match name.as_str() {
            _ if name.starts_with("zh_Hans") => Some("zh_CN"),
            _ if name.starts_with("zh_Hant") => Some("zh_TW"),
            _ if name.starts_with("sr_Latn") => Some("sr@latin"),
            _ => None,
        };
        find(&name)
            .or_else(|| script.and_then(find))
            .or_else(|| name.rsplit_once('_').and_then(|(language, _)| find(language)))
            .or_else(|| find(base))
    })
}

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Translates through `catalogue` from now on, the text of a `.po` file.
pub fn install(catalogue: &str) {
    let _ = CATALOG.set(Catalog::parse(catalogue));
    crate::i18n::set_translator(translate);
}

fn translate(context: &str, text: &str, argument: &str) -> String {
    let found = CATALOG
        .get()
        .and_then(|catalog| catalog.get(context, text))
        .unwrap_or(text);
    markup(&found.replace("%1", argument))
}

/// KDE's semantic markup as rich text: `<emphasis>` in italics, or bold when strong,
/// `<warning>` in bold, and the other tags without a look of their own.
#[must_use]
pub fn markup(text: &str) -> String {
    if !text.contains('<') {
        return text.to_string();
    }
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        result.push_str(rest.get(..start).unwrap_or_default());
        let after = rest.get(start..).unwrap_or_default();
        let Some(end) = after.find('>') else {
            result.push_str(after);
            return result;
        };
        let tag = after.get(1..end).unwrap_or_default();
        let name = tag
            .trim_start_matches('/')
            .trim_end_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or_default();
        let closing = tag.starts_with('/');
        let replacement = match (name, closing) {
            ("emphasis", false) if tag.contains("strong") => "<b>",
            ("emphasis", false) => "<i>",
            ("warning", false) => "<b>",
            ("warning", true) => "</b>",
            // Which of both closes is not known here; a closing tag that opened nothing is
            // ignored by rich text.
            ("emphasis", true) => "</i></b>",
            ("b" | "i" | "p" | "br" | "html" | "a", _) => after.get(..=end).unwrap_or_default(),
            _ => "",
        };
        result.push_str(replacement);
        rest = after.get(end + 1..).unwrap_or_default();
    }
    result.push_str(rest);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const GERMAN: &str = r#"# SPDX-License-Identifier: MIT
msgid ""
msgstr ""
"Language: de\n"

#, kde-format
msgctxt "@title:group"
msgid "Tab Bar"
msgstr "Unterfensterleiste"

msgid "Without context"
msgstr "Ohne Kontext"

msgctxt "@info"
msgid ""
"Two "
"lines\n"
msgstr ""
"Zwei "
"Zeilen\n"

#, fuzzy
msgctxt "@info"
msgid "Unsure"
msgstr "Unsicher"

msgctxt "@info"
msgid "Untranslated"
msgstr ""

msgctxt "@info"
msgid "Quote \"%1\""
msgstr "Zitat „%1“"
"#;

    #[test]
    fn entries_are_read_with_their_context() {
        let catalog = Catalog::parse(GERMAN);
        assert_eq!(catalog.get("@title:group", "Tab Bar"), Some("Unterfensterleiste"));
        assert_eq!(catalog.get("", "Without context"), Some("Ohne Kontext"));
        assert_eq!(catalog.get("@info", "Two lines\n"), Some("Zwei Zeilen\n"));
        assert_eq!(catalog.get("@info", "Quote \"%1\""), Some("Zitat „%1“"));
        assert_eq!(catalog.get("@other", "Tab Bar"), None);
        assert_eq!(catalog.len(), 4);
        assert!(!catalog.is_empty());
    }

    #[test]
    fn fuzzy_and_empty_translations_do_not_count() {
        let catalog = Catalog::parse(GERMAN);
        assert_eq!(catalog.get("@info", "Unsure"), None);
        assert_eq!(catalog.get("@info", "Untranslated"), None);
        assert!(Catalog::parse("").is_empty());
    }

    #[test]
    fn every_catalogue_of_kudari_can_be_read() {
        let german = include_str!("../../../po/de/kudari.po");
        let catalog = Catalog::parse(german);
        assert!(catalog.len() > 250, "{}", catalog.len());
        assert_eq!(catalog.get("@title:group", "Tab Bar"), Some("Unterfensterleiste"));
    }

    #[test]
    fn the_language_of_the_user_finds_its_catalogue() {
        let catalogues = [
            ("de", "de"),
            ("pt_BR", "pt_BR"),
            ("pt", "pt"),
            ("zh_TW", "zh_TW"),
            ("sr@latin", "sr@latin"),
        ];
        let pick = |languages: &[&str]| {
            let languages: Vec<String> = languages.iter().map(|language| (*language).to_string()).collect();
            choose(&languages, &catalogues)
        };
        assert_eq!(pick(&["de-AT", "en"]), Some("de"));
        assert_eq!(pick(&["pt-BR"]), Some("pt_BR"));
        assert_eq!(pick(&["pt-PT"]), Some("pt"));
        assert_eq!(pick(&["zh-Hant-TW"]), Some("zh_TW"));
        assert_eq!(pick(&["sr-Latn-RS"]), Some("sr@latin"));
        assert_eq!(pick(&["en-US", "de"]), Some("de"));
        assert_eq!(pick(&["en-US"]), None);
        assert_eq!(pick(&[]), None);
    }

    #[test]
    fn kde_markup_becomes_rich_text() {
        assert_eq!(markup("plain"), "plain");
        assert_eq!(markup("<warning>Careful</warning> now"), "<b>Careful</b> now");
        assert_eq!(markup("<emphasis>a</emphasis>"), "<i>a</i></b>");
        assert_eq!(markup("<emphasis strong='1'>a</emphasis>"), "<b>a</i></b>");
        assert_eq!(markup("Press <shortcut>Ctrl+C</shortcut>"), "Press Ctrl+C");
        assert_eq!(markup("<p>keep</p><br/>"), "<p>keep</p><br/>");
        assert_eq!(markup("a < b"), "a < b");
    }

    #[test]
    fn random_catalogues_do_no_harm() {
        for input in crate::fuzz::inputs(&[GERMAN, "<warning>x</warning>"]) {
            let _ = Catalog::parse(&input);
            let _ = markup(&input);
            let _ = unquote(&input);
        }
    }
}
