# Patched dependencies

Copies of crates from crates.io with fixes that are not released yet. `Cargo.toml` puts them
in place of the published versions through `[patch.crates-io]`. Each copy is the published
version with the change named here and nothing else, so it can be compared with the original.

| Crate | Version | Change | Upstream |
|---|---|---|---|
| `cxx-qt-build` | 0.10.0 | Links the Qt modules in a fixed order instead of the order of a `HashSet`, which differed from build to build. | [KDAB/cxx-qt#1506](https://github.com/KDAB/cxx-qt/pull/1506) |
| `qt-build-utils` | 0.10.0 | Passes `SOURCE_DATE_EPOCH` on to `rcc`, which ran with an empty environment and wrote the build time of generated files into the resources. | [KDAB/cxx-qt#1507](https://github.com/KDAB/cxx-qt/pull/1507) |
| `cxx-qt-build` | 0.10.0 | Takes a build with `DEBUG` set to `false` or `none` for one without debugging, as `is_debug` meant to: its `\|\| debug != "none"` made every build count as one for debugging, so the C++ parts of release builds lacked `QT_NO_DEBUG` and kept the asserts of Qt. | [KDAB/cxx-qt#1510](https://github.com/KDAB/cxx-qt/pull/1510) |

The first two are needed for a reproducible build of Kudari. Once a release of cxx-qt
contains all three, delete these two directories and their lines in `[patch.crates-io]`, and
run `cargo update`.

The crates are under `MIT OR Apache-2.0`, © Klarälvdalens Datakonsult AB, a KDAB Group company.

## The terminal emulator

`alacritty_terminal` 0.26.0 and the parser it uses, `vte` 0.15.0, are copied as published,
without the recorded sessions in `tests/` of `alacritty_terminal` and the example of `vte`,
which their manifests no longer name. They carry fixes of faults that freeze or end the
terminals of Kudari, or show the wrong text; `crates/core/src/upstream_tests.rs` has a test
for each, which runs against these copies.

| Crate | Change | Test |
|---|---|---|
| `alacritty_terminal` | When the stack of kitty keyboard modes is full, takes the oldest from it, not from the stack of titles, which panicked when that was empty. | `many_kitty_keyboard_modes_do_not_end_the_terminal` |
| `vte` | Hands a repeat (REP) to the handler as one call, `Handler::repeat`, which repeats as before unless the handler knows better. | `repeating_a_character_fills_no_more_than_the_screen` |
| `alacritty_terminal` | Repeats a character no more than a screen and a line, by whole lines fewer, which ends with the same screen and cursor; a character of no width at most 16 times. | `a_repeat_ends_as_writing_the_character_that_often` |
| `alacritty_terminal` | Parses output 512 bytes at a time, and once it has held the terminal for 8 ms, lets those waiting for it have it first. | `costly_output_lets_others_have_the_terminal` |
| `alacritty_terminal` | On the last line of the screen below the scrolling region, where a line feed does not move on, marks no line as going on in the next and leaves no spacer for a wide character there. | `a_wide_character_below_the_scrolling_region_keeps_the_cursor_in_the_grid`, `the_last_line_below_the_scrolling_region_goes_on_in_no_other` |
| `alacritty_terminal` | On Windows, gives an error where creating a pseudo console fails and warns where resizing it does, instead of asserting. | `a_pseudo_console_that_fails_is_an_error_not_a_panic` |

Once releases of both carry a fix, delete it here; once they carry all of them, delete both
directories and their lines in `[patch.crates-io]`, update the versions in `Cargo.toml`, and
run `cargo update`. `alacritty_terminal` is under `Apache-2.0`, `vte` under
`Apache-2.0 OR MIT`, © the authors of Alacritty.
