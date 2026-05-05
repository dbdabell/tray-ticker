# Tray Ticker

Minimal Windows 11 system-tray stock ticker: live price rendered on the tray icon (from Yahoo Finance), left-click for a small chart with **1D / 1W / 1M / 1Y** ranges, right-click for **Change ticker**, **Start with Windows**, and **Quit**.

## Prerequisites (one-time dev machine)

1. **Rust (MSVC toolchain)**  
   `winget install Rustlang.Rustup`  
   Then: `rustup default stable` (use `stable-x86_64-pc-windows-msvc` on Intel/AMD x64).

2. **Visual Studio Build Tools 2022 — C++ workload** (linker for native crates)  
   `winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"`

3. **Windows fonts** — price glyphs use **Segoe UI Bold** / **Arial Bold** from `C:\Windows\Fonts`.

## Build

This repo is usually cloned as **`…\tray-ticker\`** containing the crate in **`…\tray-ticker\tray-ticker\`** (nested same name).

**Release binary you want:**

```text
…\tray-ticker\tray-ticker\target\release\tray-ticker.exe
```

### Important: Cursor and `CARGO_TARGET_DIR`

Integrated terminals (and Cursor agent builds) sometimes set **`CARGO_TARGET_DIR`** to a temp/cache folder. Then `cargo build --release` **does not update** `tray-ticker\target\release\`, so the `.exe` there looks permanently “old”. Use:

```powershell
cd tray-ticker           # inner crate folder (next to this README)
.\scripts\build-release.ps1
```

That script clears `CARGO_TARGET_DIR` and passes **`--target-dir`** so artifacts always land in this folder’s **`target\release\`**.

Otherwise, from PowerShell outside Cursor:

```powershell
cd tray-ticker
cargo build --release
```

Typical `.exe` size: well under 10 MB with LTO + strip.

Debug run (console + logs to stderr + file):

```powershell
cargo run
```

Release run (no console window; logs in `%LOCALAPPDATA%\tray-ticker\log.txt`):

```powershell
.\target\release\tray-ticker.exe
```

## Configuration

- `%APPDATA%\tray-ticker\config.json` — symbol, last chart tab, optional flags.
- `%LOCALAPPDATA%\tray-ticker\` — logs, `show.request` (second-instance “show chart” signal).

## Data source

Uses Yahoo Finance public endpoints (`v8/finance/chart` and `v1/finance/search`). Include a realistic browser **User-Agent** (already set). Unofficial API — may change; the app surfaces errors in the tray tooltip and a retry banner.

## Single instance

A second launch signals the running instance via `%LOCALAPPDATA%\tray-ticker\show.request` and exits; the first instance opens the chart popup.

## Distribution zip

After `cargo build --release`, from the `tray-ticker` folder:

```powershell
.\scripts\make_release_zip.ps1
```

Creates `dist\tray-ticker-windows-amd64.zip` containing the `.exe`, `README.md`, and `LICENSE`.

## Screenshots

_(Add screenshots of the tray icon and chart popup here.)_

## Manual acceptance checklist

Run through these once on a Windows 11 machine after `cargo build --release`:

- Cold start: tray icon appears and loads the default symbol within a few seconds.
- Change ticker via right-click → validate bad symbols are rejected.
- Left-click: chart popup, all four ranges, Esc / focus-loss dismiss.
- Start with Windows: registry `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\TrayTicker` toggles; reboot optional.
- Second instance: creates `show.request` and the first instance opens the popup.
- Airplane mode / unplugged: error state + Retry recovers when network returns.

## License

MIT — see [LICENSE](LICENSE).
