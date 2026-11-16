# Patched dependencies

Copies of crates from crates.io with fixes that are not released yet. `Cargo.toml` puts them
in place of the published versions through `[patch.crates-io]`. Each copy is the published
version with the change named here and nothing else, so it can be compared with the original.

| Crate | Version | Change | Upstream |
|---|---|---|---|
| `cxx-qt-build` | 0.10.0 | Links the Qt modules in a fixed order instead of the order of a `HashSet`, which differed from build to build. | [KDAB/cxx-qt#1506](https://github.com/KDAB/cxx-qt/pull/1506) |
| `qt-build-utils` | 0.10.0 | Passes `SOURCE_DATE_EPOCH` on to `rcc`, which ran with an empty environment and wrote the build time of generated files into the resources. | [KDAB/cxx-qt#1507](https://github.com/KDAB/cxx-qt/pull/1507) |
| `cxx-qt-build` | 0.10.0 | Takes a build with `DEBUG` set to `false` or `none` for one without debugging, as `is_debug` meant to: its `\|\| debug != "none"` made every build count as one for debugging, so the C++ parts of release builds lacked `QT_NO_DEBUG` and kept the asserts of Qt. | not yet reported |

The first two are needed for a reproducible build of Kudari. Once a release of cxx-qt
contains all three, delete this directory and the `[patch.crates-io]` section, and run
`cargo update`.

The crates are under `MIT OR Apache-2.0`, © Klarälvdalens Datakonsult AB, a KDAB Group company.
