#!/bin/bash
# Build an AppImage for BAR - Map Editor.
# Requires: appimagetool (https://github.com/AppImage/AppImageKit)
# Usage: ./build-appimage.sh <binary-dir> <output-dir>

set -euo pipefail

BINARY_DIR="${1:-.}"
OUTPUT_DIR="${2:-.}"
VERSION="${VERSION:-0.1.0}"

APPDIR="$OUTPUT_DIR/BarEditor.AppDir"

# Create AppDir structure
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"

# Copy binaries
cp "$BINARY_DIR/bar-editor" "$APPDIR/usr/bin/"
cp "$BINARY_DIR/bar-cli" "$APPDIR/usr/bin/"
chmod +x "$APPDIR/usr/bin/bar-editor"
chmod +x "$APPDIR/usr/bin/bar-cli"

# Desktop file
cat > "$APPDIR/usr/share/applications/bar-editor.desktop" << 'EOF'
[Desktop Entry]
Name=BAR - Map Editor
Comment=Standalone map editor for Beyond All Reason
Exec=bar-editor
Icon=bar-editor
Terminal=false
Type=Application
Categories=Graphics;3DGraphics;
EOF

# AppRun
cat > "$APPDIR/AppRun" << 'EOF'
#!/bin/bash
APPDIR="$(dirname "$(readlink -f "$0")")"
exec "$APPDIR/usr/bin/bar-editor" "$@"
EOF
chmod +x "$APPDIR/AppRun"

# Symlink desktop and icon at root (AppImage requirement)
cp "$APPDIR/usr/share/applications/bar-editor.desktop" "$APPDIR/"

# Generate a placeholder icon if none exists
if [ -f "assets/icon.png" ]; then
    cp "assets/icon.png" "$APPDIR/bar-editor.png"
    cp "assets/icon.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/bar-editor.png"
else
    # Create minimal 1x1 PNG placeholder
    printf '\x89PNG\r\n\x1a\n' > "$APPDIR/bar-editor.png"
    cp "$APPDIR/bar-editor.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/bar-editor.png"
fi

# Build AppImage
ARCH=x86_64 appimagetool "$APPDIR" "$OUTPUT_DIR/bar-editor-${VERSION}-x86_64.AppImage"

echo "AppImage created: $OUTPUT_DIR/bar-editor-${VERSION}-x86_64.AppImage"
