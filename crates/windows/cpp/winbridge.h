// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

// What the Windows front end needs of Windows and of Qt beyond the shared parts, for
// `src/win.rs`. Without Windows, the global shortcut does nothing, so that the front end
// can be tried out elsewhere.
#pragma once

#include <QString>

bool rk_win_claim_instance(const QString &name);
bool rk_win_register_hotkey(const QString &sequence);
void rk_win_setup_tray(const QString &toggleText, const QString &quitText);
void rk_win_set_tray_visible(bool visible);
void rk_win_notify(const QString &title, const QString &text);
void rk_win_screenshot_later(const QString &path, int delay);
void rk_win_prepare_translucency();
void rk_win_use_style();
void rk_win_set_color_scheme(const QString &scheme);
void rk_win_attach_console();
void rk_win_watch_theme();
void rk_win_set_backdrop(bool blurred);
