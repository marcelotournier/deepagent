#!/usr/bin/env bash
set -euo pipefail

# setup_pi.sh — Set up a Raspberry Pi 4/5 for deepagent
# Installs Rust, builds deepagent, and configures the environment.

echo "=== deepagent Raspberry Pi Setup ==="
echo "Target: $(uname -m) $(uname -s)"
echo ""

# Check we're on ARM
if [[ "$(uname -m)" != "aarch64" && "$(uname -m)" != "armv7l" ]]; then
    echo "WARNING: This script is designed for Raspberry Pi (ARM64/ARMv7)."
    echo "Detected architecture: $(uname -m)"
    read -p "Continue anyway? [y/N] " -n 1 -r
    echo
    [[ $REPLY =~ ^[Yy]$ ]] || exit 1
fi

# Update system
echo "==> Updating system packages..."
sudo apt-get update -qq
sudo apt-get install -y -qq build-essential pkg-config libssl-dev curl git

# Install Rust
if command -v rustc &>/dev/null; then
    echo "==> Rust already installed: $(rustc --version)"
else
    echo "==> Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

echo "==> Rust version: $(rustc --version)"
echo "==> Cargo version: $(cargo --version)"

# Install Python and maturin (for pip install)
echo "==> Checking Python..."
if command -v python3 &>/dev/null; then
    echo "    Python: $(python3 --version)"
else
    echo "==> Installing Python3..."
    sudo apt-get install -y -qq python3 python3-pip python3-venv
fi

echo "==> Installing maturin..."
pip3 install --quiet maturin 2>/dev/null || pip install --quiet maturin

# Configure rayon for Pi's 4 cores
export RAYON_NUM_THREADS=4

# Build deepagent
echo "==> Building deepagent (release mode)..."
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

cargo build --release

echo "==> Running tests..."
cargo test --release --quiet

# Build wheel
echo "==> Building Python wheel..."
maturin build --release

# Install
WHEEL=$(ls target/wheels/deepagent-*.whl 2>/dev/null | head -1)
if [[ -n "$WHEEL" ]]; then
    echo "==> Installing wheel: $WHEEL"
    pip3 install --force-reinstall "$WHEEL" 2>/dev/null || pip install --force-reinstall "$WHEEL"
fi

# Print setup summary
echo ""
echo "=== Setup Complete ==="
echo ""
echo "Binary: $(ls -lh target/release/deepagent 2>/dev/null | awk '{print $5, $9}')"
echo ""
echo "To use deepagent:"
echo "  1. Set your API key:  export GEMINI_API_KEY='your-key-from-ai.google.dev'"
echo "  2. Run:               deepagent -p 'your prompt here'"
echo "  3. Or pipe:           echo 'explain this' | deepagent"
echo ""
echo "Recommended .bashrc additions:"
echo "  export GEMINI_API_KEY='your-key'"
echo "  export RAYON_NUM_THREADS=4"
echo "  export DEEPAGENT_LOG=info"
echo ""
echo "For verbose output:     deepagent -v -p 'your prompt'"
echo "For JSON output:        deepagent --json -p 'your prompt'"
