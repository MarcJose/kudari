// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Translations through the front end, which knows the catalogues of its platform: `KI18n` on
//! Plasma. Until a front end installs its translator, texts stay in English.

use std::sync::OnceLock;

/// Translates `text` in `context`, putting `argument` in for `%1`.
pub type Translator = fn(context: &str, text: &str, argument: &str) -> String;

static TRANSLATOR: OnceLock<Translator> = OnceLock::new();
static PLAIN_TRANSLATOR: OnceLock<Translator> = OnceLock::new();

/// Installs the translator of the front end; only the first one counts.
pub fn set_translator(translator: Translator) {
    let _ = TRANSLATOR.set(translator);
}

/// Installs a translator that gives plain text where the other one gives rich text, as
/// `KI18n` does for a text in a context such as `@info`, even without markup in it. Without
/// one, [`tr_plain`] translates as [`tr`].
pub fn set_plain_translator(translator: Translator) {
    let _ = PLAIN_TRANSLATOR.set(translator);
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

/// Translates a text of a control as plain text: screen readers read the name of a
/// control as it is, with any tags.
#[must_use]
pub fn tr_plain(context: &str, text: &str) -> String {
    tr_plain_arg(context, text, "")
}

/// Translates a text with a `%1` placeholder as plain text; see [`tr_plain`].
#[must_use]
pub fn tr_plain_arg(context: &str, text: &str, argument: &str) -> String {
    match PLAIN_TRANSLATOR.get() {
        Some(translator) => translator(context, text, argument),
        None => tr_arg(context, text, argument),
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

    #[test]
    fn plain_texts_translate_as_the_others_without_a_translator_for_them() {
        assert_eq!(tr_plain("@info", "Quit Kudari?"), tr("@info", "Quit Kudari?"));
        assert_eq!(
            tr_plain_arg("@info", "Bell in session \"%1\".", "<b>"),
            tr_arg("@info", "Bell in session \"%1\".", "<b>")
        );
    }
}
