# RedStart

A portable Rust terminal app for launching selected applications on Windows and Linux. It stores `redstart.json` beside the executable. On first launch, it creates an empty configuration and shows the app list.

The packaged `RedStart.exe` in this project directory is ready to run on Windows and uses the adjacent `redstart.json`. The Linux executable must be built from this source on Linux.

## Build

Rust 1.85 or later is required. From this directory:

```sh
cargo build --release
```

On Windows, copy `target/release/redstart.exe` to `RedStart.exe` beside your configuration after building, then run it there. The supplied icon is embedded in the executable. The Windows build uses a static C runtime. On Linux, run `target/release/redstart`; `assets/redstart.png` is the matching desktop icon. To register a launcher in your Linux desktop menu, run `sh install-desktop.sh` after building.

Place the executable in a folder where you can write its JSON configuration. The app does not install a service or require administrator rights. A terminal with ANSI color and arrow key support is needed for the interactive view.

## Keyboard interface

- `↑` / `↓`: highlight an app or the Add, Relocate, Remove, Run Batch and Close, or Close row.
- `Enter` on an app: start it when off, or stop it when running. Stop terminates all running processes with that exact executable path; unsaved work in those apps can be lost.
- `Tab`: switch the selected app between Individual and Batch.
- `F2`: rename the selected app. The input starts with its current name.
- `Enter` on Relocate or Remove: open an app list, navigate to the desired row, and press Enter. Remove shows No and Yes; use `←` / `→` and `Enter` to confirm, or `Esc` to cancel. Relocate opens the file browser.
- `Enter` on a bottom row: add an app, run Batch and Close, or close. Add asks for a name and opens a keyboard file browser. The browser starts at the system drive root on Windows or `/` on Linux. In the browser, Enter opens a directory or selects a file; Backspace goes to the parent directory; Esc cancels. At a Windows drive root, type another drive letter to switch drives.
- `Esc`: close the main view.

On startup, RedStart checks each saved application path. A missing or unusable path shows `Not Found` in the Setting column. This is automatic; the saved Individual or Batch setting returns after relocation. Enter or Tab on a `Not Found` row opens the relocator. Batch starts every configured Batch app that is not already running. Successful completion shows a five second auto-close countdown; any key closes it immediately. Launch errors are displayed until Enter is pressed. Status is based on the exact executable path, refreshed every two seconds. The file chooser accepts executable files (`.exe` and `.com` on Windows, files with an executable permission bit on Linux).

While RedStart starts or stops an app or saves its configuration, the interface dims and shows `Working...`. Other controls are blocked until completion. `Esc` force quits RedStart during this state.

## CLI

```text
redstart list
redstart add <name> <path> [--batch]
redstart edit <number> [--name <name>] [--path <path>] [--mode individual|batch]
redstart remove <number> [--yes]
redstart activate <number>
redstart stop <number>
redstart batch
```

Row numbers are one based. Set `REDSTART_CONFIG` to a JSON file path to use a different configuration, for example while testing. Path arguments containing spaces must be quoted by the shell.
