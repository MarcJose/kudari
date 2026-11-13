// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

// Sends pointer and key events through KWin's fake input protocol, for tests/gui/run.sh.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wayland-client.h>
#include "fake-input-client.h"

static struct org_kde_kwin_fake_input *fake;

static void global(void *data, struct wl_registry *r, uint32_t name, const char *iface, uint32_t version)
{
    if (!strcmp(iface, "org_kde_kwin_fake_input"))
        fake = wl_registry_bind(r, name, &org_kde_kwin_fake_input_interface, version < 4 ? version : 4);
}
static void global_remove(void *d, struct wl_registry *r, uint32_t n)
{
    (void)d;
    (void)r;
    (void)n;
}
static const struct wl_registry_listener listener = {global, global_remove};

int main(int argc, char **argv)
{
    struct wl_display *display = wl_display_connect(NULL);
    if (!display) { fprintf(stderr, "no display\n"); return 2; }
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &listener, NULL);
    wl_display_roundtrip(display);
    if (!fake) { fprintf(stderr, "fake input not offered\n"); return 3; }
    org_kde_kwin_fake_input_authenticate(fake, "kudari-test", "test");
    const char *cmd = argv[1];
    if (!strcmp(cmd, "click") || !strcmp(cmd, "rclick") || !strcmp(cmd, "dbl") || !strcmp(cmd, "mclick")) {
        double x = atof(argv[2]), y = atof(argv[3]);
        uint32_t b = !strcmp(cmd, "rclick") ? 273 : !strcmp(cmd, "mclick") ? 274 : 272;
        org_kde_kwin_fake_input_pointer_motion_absolute(fake, wl_fixed_from_double(x), wl_fixed_from_double(y));
        wl_display_roundtrip(display); usleep(150000);
        int n = !strcmp(cmd, "dbl") ? 2 : 1;
        for (int i = 0; i < n; ++i) {
            org_kde_kwin_fake_input_button(fake, b, 1); wl_display_roundtrip(display); usleep(40000);
            org_kde_kwin_fake_input_button(fake, b, 0); wl_display_roundtrip(display); usleep(40000);
        }
    } else if (!strcmp(cmd, "drag")) {
        double x1 = atof(argv[2]), y1 = atof(argv[3]), x2 = atof(argv[4]), y2 = atof(argv[5]);
        org_kde_kwin_fake_input_pointer_motion_absolute(fake, wl_fixed_from_double(x1), wl_fixed_from_double(y1));
        wl_display_roundtrip(display); usleep(150000);
        org_kde_kwin_fake_input_button(fake, 272, 1); wl_display_roundtrip(display); usleep(80000);
        for (int i = 1; i <= 20; ++i) {
            org_kde_kwin_fake_input_pointer_motion_absolute(fake, wl_fixed_from_double(x1 + (x2 - x1) * i / 20), wl_fixed_from_double(y1 + (y2 - y1) * i / 20));
            wl_display_roundtrip(display); usleep(25000);
        }
        org_kde_kwin_fake_input_button(fake, 272, 0); wl_display_roundtrip(display); usleep(80000);
    } else if (!strcmp(cmd, "wheel")) {
        org_kde_kwin_fake_input_pointer_motion_absolute(fake, wl_fixed_from_double(atof(argv[2])), wl_fixed_from_double(atof(argv[3])));
        wl_display_roundtrip(display); usleep(150000);
        org_kde_kwin_fake_input_axis(fake, 0, wl_fixed_from_double(atof(argv[4])));
        wl_display_roundtrip(display); usleep(100000);
    } else if (!strcmp(cmd, "keydown")) {
        org_kde_kwin_fake_input_keyboard_key(fake, atoi(argv[2]), 1);
    } else if (!strcmp(cmd, "keyup")) {
        org_kde_kwin_fake_input_keyboard_key(fake, atoi(argv[2]), 0);
    } else if (!strcmp(cmd, "hold")) {
        org_kde_kwin_fake_input_pointer_motion_absolute(fake, wl_fixed_from_double(atof(argv[2])), wl_fixed_from_double(atof(argv[3])));
        wl_display_roundtrip(display); usleep(150000);
        org_kde_kwin_fake_input_button(fake, 272, 1); wl_display_roundtrip(display);
        usleep(atoi(argv[4]) * 1000);
        org_kde_kwin_fake_input_button(fake, 272, 0); wl_display_roundtrip(display); usleep(80000);
    } else if (!strcmp(cmd, "moddrag")) {
        double x1 = atof(argv[2]), y1 = atof(argv[3]), x2 = atof(argv[4]), y2 = atof(argv[5]);
        for (int i = 6; i < argc; ++i) { org_kde_kwin_fake_input_keyboard_key(fake, atoi(argv[i]), 1); wl_display_roundtrip(display); usleep(30000); }
        org_kde_kwin_fake_input_pointer_motion_absolute(fake, wl_fixed_from_double(x1), wl_fixed_from_double(y1));
        wl_display_roundtrip(display); usleep(150000);
        org_kde_kwin_fake_input_button(fake, 272, 1); wl_display_roundtrip(display); usleep(80000);
        for (int i = 1; i <= 12; ++i) {
            org_kde_kwin_fake_input_pointer_motion_absolute(fake, wl_fixed_from_double(x1 + (x2 - x1) * i / 12), wl_fixed_from_double(y1 + (y2 - y1) * i / 12));
            wl_display_roundtrip(display); usleep(40000);
        }
        org_kde_kwin_fake_input_button(fake, 272, 0); wl_display_roundtrip(display); usleep(80000);
        for (int i = argc - 1; i >= 6; --i) { org_kde_kwin_fake_input_keyboard_key(fake, atoi(argv[i]), 0); wl_display_roundtrip(display); usleep(30000); }
    } else if (!strcmp(cmd, "ctrlhold")) {
        org_kde_kwin_fake_input_pointer_motion_absolute(fake, wl_fixed_from_double(atof(argv[2])), wl_fixed_from_double(atof(argv[3])));
        wl_display_roundtrip(display); usleep(300000);
        org_kde_kwin_fake_input_keyboard_key(fake, 29, 1); wl_display_roundtrip(display);
        usleep(atoi(argv[4]) * 1000);
        org_kde_kwin_fake_input_keyboard_key(fake, 29, 0); wl_display_roundtrip(display); usleep(100000);
    } else if (!strcmp(cmd, "move")) {
        org_kde_kwin_fake_input_pointer_motion_absolute(fake, wl_fixed_from_double(atof(argv[2])), wl_fixed_from_double(atof(argv[3])));
        wl_display_roundtrip(display); usleep(100000);
    } else if (!strcmp(cmd, "key")) {
        for (int i = 2; i < argc; ++i) {
            int code = atoi(argv[i]);
            org_kde_kwin_fake_input_keyboard_key(fake, code, 1); wl_display_roundtrip(display); usleep(30000);
        }
        for (int i = argc - 1; i >= 2; --i) {
            org_kde_kwin_fake_input_keyboard_key(fake, atoi(argv[i]), 0); wl_display_roundtrip(display); usleep(30000);
        }
    }
    wl_display_roundtrip(display);
    wl_display_disconnect(display);
    return 0;
}
