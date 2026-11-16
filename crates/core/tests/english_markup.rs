// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The translations of the Windows front end in English, in a process of their own: the
//! translator can be installed only once.

use kudari_core::{gettext, i18n};

/// English as the language of Windows has no catalogue: the texts stay English, and the
/// markup of KDE in them still becomes rich text instead of tags that the dialogs leave out.
#[test]
fn english_without_a_catalogue_turns_markup_into_rich_text() {
    let languages = ["en-US", "de-DE"].map(String::from);
    gettext::install_for(&languages, &[("de", include_str!("../../../po/de/kudari.po"))]);
    assert_eq!(
        i18n::tr(
            "@info",
            "<warning>Several sessions are open. Quitting ends all of them.</warning>"
        ),
        "<b>Several sessions are open. Quitting ends all of them.</b>"
    );
    let protected = i18n::tr(
        "@info",
        "<warning>Several sessions are open, <emphasis>and some of them are protected from closing.</emphasis> Quitting ends all of them.</warning>",
    );
    assert!(
        protected.starts_with("<b>Several sessions are open, <i>and some"),
        "{protected}"
    );
}
