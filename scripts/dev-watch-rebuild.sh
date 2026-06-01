#!/usr/bin/env bash
# Rebuild-and-relaunch step for the BME dev watch loop.
#
# Invoked by `cargo watch` on every source change. The running editor
# holds `target/debug/bar-editor.exe` open, so the linker can't
# overwrite it mid-rebuild -- that's the failure a plain `cargo watch
# -x run` hits on Windows. Killing the editor first releases the lock;
# launching the rebuilt binary detached lets cargo-watch treat this
# command as finished so it returns to watching instead of blocking on
# the running editor.

# Release the exe lock held by any running editor. taskkill needs the
# doubled slashes under mingw bash so they aren't mangled into paths.
taskkill //IM bar-editor.exe //F >/dev/null 2>&1 || true
# Brief pause so Windows actually releases the file handle before the
# linker writes the new binary.
sleep 0.3

if cargo build -p bar-app; then
    # `cmd start` launches truly detached so the editor outlives this
    # script (and this cargo-watch command invocation).
    cmd //c start "" "target\\debug\\bar-editor.exe"
    echo "[dev-watch] build ok -- relaunched bar-editor"
else
    echo "[dev-watch] build FAILED -- editor not relaunched (fix the error and save again)"
fi
