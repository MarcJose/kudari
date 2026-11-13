// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The settings in `KConfig`, the file `kudarirc` of KDE Plasma.

use crate::kde::{qs, rk_config_read, rk_config_write};
use crate::settings::Store;

/// The configuration of the application, as `KConfig` keeps it.
pub struct KConfig;

impl Store for KConfig {
    fn read(&self, group: &str, key: &str, default: &str) -> String {
        rk_config_read(&qs(group), &qs(key), &qs(default)).to_string()
    }

    fn write(&self, group: &str, key: &str, value: &str) {
        rk_config_write(&qs(group), &qs(key), &qs(value));
    }
}
