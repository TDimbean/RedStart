# RedStart

A portable Rust terminal app for launching selected applications on Windows and Linux. It stores `redstart.json` beside the executable. On first launch, it creates an empty configuration and shows the app list.

The packaged `RedStart.exe` is ready to run on Windows. The Linux executable must be built from this source on Linux.

## Build

Rust 1.85 or later is required. From this directory:

```sh
cargo build --release
```

On Windows, run `target/release/redstart.exe`. The supplied icon is embedded in the executable. The Windows build uses a static C runtime. On Linux, run `target/release/redstart`; `assets/redstart.png` is the matching desktop icon. To register a launcher in your Linux desktop menu, run `sh install-desktop.sh` after building.

Place the executable in a folder where you can write its JSON configuration. The app does not install a service or require administrator rights. A terminal with ANSI color and arrow key support is needed for the interactive view.

## Keyboard interface

- `↑` / `↓`: highlight an app or the Add, Batch, or Cancel row.
- `E`: edit the highlighted name, location, or Individual/Batch setting.
- `A`: activate the highlighted app now. Running apps are not started twice.
- `R` or `Delete`: remove the highlighted app after confirmation.
- `Enter`: use Add, Batch, or Cancel. Add asks for a name and opens a keyboard file browser. In the browser, Enter opens a directory or selects a file; Backspace goes to the parent directory; Esc cancels.
- `Q` or `Esc`: close the main view.

Batch starts every configured Batch app that is not already running. Successful completion shows a five second auto-close countdown. Launch errors are displayed until Enter is pressed. Status is based on the exact executable path, refreshed every two seconds. The file chooser accepts executable files (`.exe` and `.com` on Windows, files with an executable permission bit on Linux).

## CLI

```text
redstart list
redstart add <name> <path> [--batch]
redstart edit <number> [--name <name>] [--path <path>] [--mode individual|batch]
redstart remove <number> [--yes]
redstart activate <number>
redstart batch
```

Row numbers are one based. Set `REDSTART_CONFIG` to a JSON file path to use a different configuration, for example while testing. Path arguments containing spaces must be quoted by the shell.
