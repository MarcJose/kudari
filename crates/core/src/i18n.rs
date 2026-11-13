// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Translations through the front end, which knows the catalogues of its platform: `KI18n` on
//! Plasma. Until a front end installs its translator, texts stay in English.

use std::sync::OnceLock;

/// Translates `text` in `context`, putting `argument` in for `%1`.
pub type Translator = fn(context: &str, text: &str, argument: &str) -> String;

static TRANSLATOR: OnceLock<Translator> = OnceLock::new();

/// Installs the translator of the front end; only the first one counts.
pub fn set_translator(translator: Translator) {
    let _ = TRANSLATOR.set(translator);
}

/// Translates a text of the user interface.
#[must_use]
pub fn tr(context: &str, text: &str) -> String {
    tr_arg(context, text, "")
}

/// Translates a text with a `%1` placeholder.
#[must_use]
pub fn tr_arg(context: &str, text: &str, argument: &str) -> String {
    match TRANSLATOR.get() {
        Some(translator) => translator(context, text, argument),
        None => text.replace("%1", argument),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_stay_english_without_a_translator() {
        assert_eq!(tr("@info", "Quit Kudari?"), "Quit Kudari?");
        assert_eq!(
            tr_arg("@info", "Bell in session \"%1\".", "Shell"),
            "Bell in session \"Shell\"."
        );
    }
}
