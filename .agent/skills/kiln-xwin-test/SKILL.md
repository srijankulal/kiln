---
name: kiln-xwin-test
description: Provides exact procedures and commands for verifying Linux unit tests alongside cross-target Windows compilation using cargo-xwin.
---

# Kiln Cross-Compilation & Verification Guide

Kiln is developed on an Ubuntu Linux environment and cross-compiled to `x86_64-pc-windows-msvc` using `cargo-xwin`.

## Cross-Compilation Architecture

1. **Toolchain Environment:**
   - Linux host toolchain: `stable-x86_64-unknown-linux-gnu` with `llvm-tools-preview`.
   - Cross target: `x86_64-pc-windows-msvc`.
   - Tool: `cargo-xwin` which downloads and manages the Microsoft CRT and Windows SDK headers/libraries in an isolated cache.
   - Env var: `XWIN_ACCEPT_LICENSE=1` must be set.

2. **Verification Commands:**
   - Format check:
     ```bash
     cargo fmt --all -- --check
     ```
   - Linux unit and property tests (L0 and L1):
     ```bash
     cargo test --workspace --locked
     # Or with nextest:
     cargo nextest run --workspace --locked
     ```
   - Windows cross-compilation check (Clippy):
     ```bash
     cargo xwin clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc -- -D warnings
     ```
   - Full Windows Release build:
     ```bash
     cargo xwin build --release --locked --target x86_64-pc-windows-msvc
     ```

3. **Smoke Testing Windows Binaries on Linux:**
   - Windows console binaries can be smoke-tested under `wine64`:
     ```bash
     wine64 target/x86_64-pc-windows-msvc/release/kiln-daemon.exe --help
     ```
   - Never attempt to test real GUI/display switching or process suspension under Wine. Keep those strictly behind `Fake*` traits in Linux tests.
