# Third-party components

mihomo-x distributes an unmodified Mihomo v1.19.32 executable as an independent child process.

- Mihomo: GPL-3.0; license in `licenses/mihomo-LICENSE`. Corresponding versioned source: https://github.com/MetaCubeX/mihomo/tree/v1.19.32 and https://github.com/MetaCubeX/mihomo/archive/refs/tags/v1.19.32.tar.gz.
- Windows Reactor and Rust for Windows: Microsoft, MIT OR Apache-2.0. MIT license in `licenses/windows-rs-MIT`.
- Windows App SDK runtime and Microsoft VC++ runtime: Microsoft redistributables, supplied unchanged from the SDK and Visual Studio redist directories. Their applicable Microsoft license terms apply.
- Other Rust dependencies and exact versions are recorded in Cargo.lock. Their upstream license metadata is available in the corresponding crates.io packages.

The application's own license is not yet selected. This release is distributed through a private repository; no open-source license grant for the application is implied.
