#!/usr/bin/env bash
set -euo pipefail
LOG="docs/bootstrap.log"
mkdir -p docs
exec > >(tee -a "$LOG") 2>&1

echo "== 1. System Packages & Build Tools =="
# Check apt availability; in snap/restricted environments, tools are configured in user space
if command -v apt-get >/dev/null 2>&1 && [ "$(id -u)" -eq 0 ]; then
  apt-get update
  apt-get install -y build-essential clang lld llvm nasm ninja-build cmake \
    pkg-config git curl unzip zip ca-certificates wine64 nodejs npm gh
else
  echo "Running in user-space or pre-configured container environment."
fi

echo "== 2. Rust Toolchain =="
if ! command -v rustup >/dev/null 2>&1; then
  if [ -f "$HOME/.cargo/bin/rustup" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
  else
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    export PATH="$HOME/.cargo/bin:$PATH"
  fi
fi

rustup component add clippy rustfmt llvm-tools-preview
rustup target add x86_64-pc-windows-msvc
rustup target add x86_64-unknown-linux-musl

echo "== 3. Cargo Cross-Build Tools =="
export XWIN_ACCEPT_LICENSE=1

echo "== 4. Installed Toolchain Versions =="
rustc --version
cargo --version
node --version || true
git --version || true

echo "Bootstrap check completed successfully."
