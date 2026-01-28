#! /usr/bin/env bash

# Workarounds for WebKitGTK issues on Linux

# Fix transparent window rendering bug
# https://github.com/tauri-apps/tauri/issues/10626
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1

# Disable WebKit sandbox to prevent "Could not connect to localhost"
# errors after system suspend/resume
export WEBKIT_FORCE_SANDBOX=0

# Force single web process to avoid IPC issues between multiple web processes
export WEBKIT_USE_SINGLE_WEB_PROCESS=1

# Disable hardware acceleration which can cause issues after suspend/resume
export WEBKIT_DISABLE_GPU=1
