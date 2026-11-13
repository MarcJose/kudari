// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

// Thin C++ layer around KDE Frameworks that have no Rust bindings.
#pragma once

#include <QQuickItem>
#include <QString>

class QInputMethodEvent;
class QFont;
class QPainter;
class QQmlApplicationEngine;

void rk_command_line(const QString &version);
void rk_init();
void rk_setup_qml_i18n(QQmlApplicationEngine &engine);
QString rk_theme_colors(const QString &theme);
QString rk_global_themes_json();
void rk_popup_drop_menu(int x, int y, int terminal, const QString &paths, const QString &directory);
void rk_finish_startup(bool showTray);

QString rk_config_read(const QString &group, const QString &key, const QString &fallback);
void rk_config_write(const QString &group, const QString &key, const QString &value);

QString rk_shortcuts_json();
QString rk_highlight_color();
QString rk_available_screen_rect(const QString &output);
QString rk_active_output_name();
QString rk_output_names();

void rk_set_action_state(const QString &name, bool checked, bool enabled);
void rk_set_action_text(const QString &name, const QString &text);
void rk_set_menu_state(const QString &json);
void rk_set_tray_visible(bool visible);
void rk_reload_shortcuts();
QString rk_toggle_shortcut();
QString rk_foreign_toggle_shortcut();
void rk_set_toggle_shortcut(const QString &sequence);

void rk_popup_main_menu(int x, int y);
void rk_popup_session_menu(int x, int y);
void rk_popup_tab_menu(int x, int y, const QString &json);
void rk_popup_terminal_menu(int x, int y, bool canCopy, const QString &link, bool autoSaving);

void rk_about(const QString &what);
void rk_configure_keys();
void rk_configure_notifications();
void rk_notify(const QString &event, const QString &text);
void rk_slide_window();
void rk_ask_agent(const QString &agent, const QString &title, const QString &text, const QString &allow, const QString &deny);
void rk_close_agent_question();
void rk_ask_question(int id, const QString &text, const QString &title, int continueKind);


QString rk_tr(const QString &context, const QString &text, const QString &argument);
QString rk_action_text(const QString &name);
void rk_open_url(const QString &url);
void rk_web_search(const QString &text);
void rk_set_blur(bool enabled);

void rk_test_key(const QString &sequence);
void rk_test_input_method(const QString &preedit, const QString &commit);
QString rk_test_window_state();
void rk_test_grab(const QString &path);
QString rk_test_agent_question();
void rk_test_answer_agent(bool allow);
