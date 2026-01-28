#! /usr/bin/env bash

# Workarounds for WebKitGTK issues on Linux

# Fix transparent window rendering bug
# https://github.com/tauri-apps/tauri/issues/10626
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1

# Disable WebKit sandbox to prevent "Could not connect to localhost"
# errors after system suspend/resume
export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1

# Force single web process to avoid IPC issues between multiple web processes
export WEBKIT_USE_SINGLE_WEB_PROCESS=1

# Disable hardware acceleration which can cause issues after suspend/resume
export WEBKIT_DISABLE_GPU=1

gsettings get org.gnome.desktop.interface gtk-theme 2> /dev/null | grep -qi "dark" && GTK_THEME_VARIANT="dark" || GTK_THEME_VARIANT="light"
APPIMAGE_GTK_THEME="${APPIMAGE_GTK_THEME:-"Adwaita:$GTK_THEME_VARIANT"}" # Allow user to override theme (discouraged)

export APPDIR="${APPDIR:-"$(dirname "$(realpath "$0")")"}" # Workaround to run extracted AppImage
export GTK_DATA_PREFIX="$APPDIR"
export GTK_THEME="$APPIMAGE_GTK_THEME" # Custom themes are broken
export GDK_BACKEND=x11 # Crash with Wayland backend on Wayland - We tested it without it and ended up with this: https://github.com/tauri-apps/tauri/issues/8541
export XDG_DATA_DIRS="$APPDIR/usr/share:/usr/share:$XDG_DATA_DIRS" # g_get_system_data_dirs() from GLib
export GSETTINGS_SCHEMA_DIR="$APPDIR//usr/share/glib-2.0/schemas"
export GTK_EXE_PREFIX="$APPDIR//usr"
export GTK_PATH="$APPDIR//usr/lib/x86_64-linux-gnu/gtk-3.0:/usr/lib64/gtk-3.0:/usr/lib/x86_64-linux-gnu/gtk-3.0"
export GTK_IM_MODULE_FILE="$APPDIR//usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache"

export GDK_PIXBUF_MODULE_FILE="$APPDIR//usr/lib/x86_64-linux-gnu/gdk-pixbuf-2.0/2.10.0/loaders.cache"
export GIO_EXTRA_MODULES="$APPDIR/usr/lib/x86_64-linux-gnu/gio/modules"
