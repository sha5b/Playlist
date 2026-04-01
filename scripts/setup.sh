#!/usr/bin/env bash
set -euo pipefail

echo "=== Playlist Dev Environment Setup (Unix) ==="
echo ""

# --- Node.js ---
if command -v node &>/dev/null; then
    echo "[OK] Node.js $(node --version)"
else
    echo "[!!] Node.js not found."
    echo "     Install via https://nodejs.org/ or:"
    echo "       curl -fsSL https://fnm.vercel.app/install | bash"
    echo "       fnm install --lts"
    exit 1
fi

# --- npm ---
if command -v npm &>/dev/null; then
    echo "[OK] npm $(npm --version)"
else
    echo "[!!] npm not found. It should come with Node.js."
    exit 1
fi

# --- Rust ---
if command -v rustc &>/dev/null; then
    echo "[OK] Rust $(rustc --version | cut -d' ' -f2)"
else
    echo "[..] Rust not found. Installing via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    echo "[OK] Rust $(rustc --version | cut -d' ' -f2)"
fi

# --- Platform-specific dependencies ---
OS="$(uname -s)"
case "$OS" in
    Linux)
        echo ""
        echo "[..] Checking Linux system dependencies for Tauri..."
        if command -v apt-get &>/dev/null; then
            MISSING=()
            for pkg in libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf; do
                if ! dpkg -s "$pkg" &>/dev/null 2>&1; then
                    MISSING+=("$pkg")
                fi
            done
            if [ ${#MISSING[@]} -gt 0 ]; then
                echo "     Installing: ${MISSING[*]}"
                sudo apt-get update
                sudo apt-get install -y "${MISSING[@]}"
            else
                echo "[OK] All Linux system dependencies installed"
            fi
        elif command -v dnf &>/dev/null; then
            sudo dnf install -y webkit2gtk4.1-devel gtk3-devel libayatana-appindicator-gtk3-devel librsvg2-devel patchelf
        elif command -v pacman &>/dev/null; then
            sudo pacman -S --needed webkit2gtk-4.1 gtk3 libayatana-appindicator librsvg patchelf
        else
            echo "[!!] Unknown Linux package manager. Install Tauri deps manually:"
            echo "     https://v2.tauri.app/start/prerequisites/#linux"
        fi
        ;;
    Darwin)
        echo ""
        echo "[..] Checking macOS dependencies..."
        if ! command -v xcode-select &>/dev/null || ! xcode-select -p &>/dev/null; then
            echo "     Installing Xcode Command Line Tools..."
            xcode-select --install
        else
            echo "[OK] Xcode CLT installed"
        fi
        ;;
    MINGW*|MSYS*|CYGWIN*)
        echo ""
        echo "[..] Windows detected via Git Bash / MSYS2."
        echo "     Ensure you have:"
        echo "       - Visual Studio Build Tools (C++ workload)"
        echo "       - WebView2 (included in Windows 10/11)"
        echo "     https://v2.tauri.app/start/prerequisites/#windows"
        ;;
esac

# --- Install npm dependencies ---
echo ""
echo "[..] Installing npm dependencies..."
npm install

# --- Verify Tauri CLI ---
echo ""
if npx tauri --version &>/dev/null; then
    echo "[OK] Tauri CLI $(npx tauri --version)"
else
    echo "[!!] Tauri CLI not found. Run: npm install -D @tauri-apps/cli"
    exit 1
fi

# --- Pre-build Rust dependencies (warm cache) ---
echo ""
echo "[..] Pre-building Rust dependencies (this may take a few minutes on first run)..."
cd src-tauri && cargo check && cd ..

echo ""
echo "=== Setup complete! ==="
echo ""
echo "Run the app:  npx tauri dev"
echo "Build:        npx tauri build"
