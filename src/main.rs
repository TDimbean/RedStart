mod model;
mod terminal;

use model::{Config, Entry, Mode};
use std::{env, io::{self, Write}, path::{Path, PathBuf}, time::{Duration, Instant}};
use terminal::{Key, Terminal, read_key};

fn main() {
    if let Err(error) = run() { eprintln!("Error: {error}"); std::process::exit(1); }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if matches!(args.first().map(String::as_str), Some("help" | "--help" | "-h")) { help(); return Ok(()); }
    let config_path = model::config_path().map_err(|e| e.to_string())?;
    let mut config = model::load(&config_path)?;
    if args.is_empty() { return tui(&config_path, &mut config); }
    cli(&args, &config_path, &mut config)
}

fn help() {
    println!("RedStart\n\nUSAGE\n  redstart [command]\n\nCOMMANDS\n  list\n  add <name> <path> [--batch]\n  edit <number> [--name <name>] [--path <path>] [--mode individual|batch]\n  remove <number> [--yes]\n  activate <number>\n  batch\n  help\n\nNo command opens the keyboard interface. Data is saved in redstart.json next to the executable.");
}

fn index(raw: &str, len: usize) -> Result<usize, String> {
    let number: usize = raw.parse().map_err(|_| "Expected a row number".to_string())?;
    if number == 0 || number > len { return Err(format!("Row must be between 1 and {len}")); }
    Ok(number - 1)
}

fn cli(args: &[String], path: &Path, config: &mut Config) -> Result<(), String> {
    match args[0].as_str() {
        "list" => {
            let processes = model::running_paths();
            for (i, entry) in config.entries.iter().enumerate() {
                println!("{:>3}  {:<28} {:<11} {:<7} {}", i + 1, entry.name, entry.mode, if model::is_running(entry, &processes) { "RUNNING" } else { "OFF" }, entry.path.display());
            }
            Ok(())
        }
        "add" => {
            if args.len() < 3 || args.len() > 4 || (args.len() == 4 && args[3] != "--batch") { return Err("Usage: add <name> <path> [--batch]".into()); }
            let mode = if args.len() == 4 { Mode::Batch } else { Mode::Individual };
            let entry = model::validated_entry(&args[1], Path::new(&args[2]), mode)?;
            config.entries.push(entry);
            model::save(path, config)?;
            println!("Added row {}", config.entries.len());
            Ok(())
        }
        "edit" => {
            if args.len() < 4 { return Err("Usage: edit <number> [--name <name>] [--path <path>] [--mode individual|batch]".into()); }
            let i = index(&args[1], config.entries.len())?;
            let mut name = config.entries[i].name.clone();
            let mut target = config.entries[i].path.clone();
            let mut mode = config.entries[i].mode;
            let mut at = 2;
            while at < args.len() {
                if at + 1 >= args.len() { return Err(format!("Missing value for {}", args[at])); }
                match args[at].as_str() {
                    "--name" => name = args[at + 1].clone(),
                    "--path" => target = PathBuf::from(&args[at + 1]),
                    "--mode" => mode = parse_mode(&args[at + 1])?,
                    option => return Err(format!("Unknown option: {option}")),
                }
                at += 2;
            }
            config.entries[i] = model::validated_entry(&name, &target, mode)?;
            model::save(path, config)?;
            println!("Updated row {}", i + 1);
            Ok(())
        }
        "remove" => {
            if args.len() < 2 || args.len() > 3 || (args.len() == 3 && args[2] != "--yes") { return Err("Usage: remove <number> [--yes]".into()); }
            let i = index(&args[1], config.entries.len())?;
            if args.len() == 2 {
                print!("Remove {}? [y/N] ", config.entries[i].name);
                io::stdout().flush().map_err(|e| e.to_string())?;
                let mut answer = String::new();
                io::stdin().read_line(&mut answer).map_err(|e| e.to_string())?;
                if !answer.trim().eq_ignore_ascii_case("y") { println!("Canceled"); return Ok(()); }
            }
            config.entries.remove(i);
            model::save(path, config)?;
            println!("Removed row {}", i + 1);
            Ok(())
        }
        "activate" => {
            if args.len() != 2 { return Err("Usage: activate <number>".into()); }
            let i = index(&args[1], config.entries.len())?;
            if model::is_running(&config.entries[i], &model::running_paths()) { println!("Already running: {}", config.entries[i].name); }
            else { model::launch(&config.entries[i])?; println!("Started: {}", config.entries[i].name); }
            Ok(())
        }
        "batch" => {
            if args.len() != 1 { return Err("Usage: batch".into()); }
            batch_result(model::launch_batch(config), None)
        }
        other => Err(format!("Unknown command: {other}. Run help for usage.")),
    }
}

fn parse_mode(raw: &str) -> Result<Mode, String> {
    match raw.to_ascii_lowercase().as_str() {
        "individual" => Ok(Mode::Individual), "batch" => Ok(Mode::Batch),
        _ => Err("Mode must be individual or batch".into()),
    }
}

fn line(entry: &Entry, selected: bool, running: bool) -> String {
    let name = crop(&entry.name, 32);
    let status = if running { "\x1b[92mRUNNING\x1b[0m" } else { "\x1b[91mOFF\x1b[0m" };
    format!("{} {:<32} {:<13} {}", if selected { "\x1b[95m▶\x1b[0m" } else { " " }, name, entry.mode, status)
}

fn crop(value: &str, max: usize) -> String {
    let count = value.chars().count();
    if count <= max { value.to_string() } else { format!("{}…", value.chars().take(max - 1).collect::<String>()) }
}

fn tui(path: &Path, config: &mut Config) -> Result<(), String> {
    let mut term = Terminal::new().map_err(|e| e.to_string())?;
    let mut selected = 0usize;
    let mut message = if config.entries.is_empty() { "Ready. Add an app to get started." } else { "Ready." }.to_string();
    let mut processes = model::running_paths();
    let mut last_scan = Instant::now();
    loop {
        let total = config.entries.len() + 3;
        selected = selected.min(total - 1);
        let mut frame = vec![
            "\x1b[1;91m  ⚡ REDSTART\x1b[0m  Application launcher".to_string(),
            "".to_string(),
            "  NAME                             SETTING       STATUS".to_string(),
            "  ─────────────────────────────────────────────────────────────".to_string(),
        ];
        let start = if selected < 9 { 0 } else { selected.saturating_sub(8).min(config.entries.len().saturating_sub(9)) };
        if start > 0 { frame.push(format!("  ↑ {} earlier rows", start)); }
        for i in start..config.entries.len().min(start + 9) {
            frame.push(line(&config.entries[i], selected == i, model::is_running(&config.entries[i], &processes)));
        }
        if start + 9 < config.entries.len() { frame.push(format!("  ↓ {} more rows", config.entries.len() - start - 9)); }
        frame.extend([
            "".into(),
            format!("{}                     + Add a new app", if selected == config.entries.len() { "\x1b[95m▶\x1b[0m" } else { " " }),
            format!("{}                ▶ Start all BATCH apps", if selected == config.entries.len() + 1 { "\x1b[95m▶\x1b[0m" } else { " " }),
            format!("{}                         Cancel / Close", if selected == config.entries.len() + 2 { "\x1b[95m▶\x1b[0m" } else { " " }),
            "".into(),
            "  ↑↓ Navigate   Enter Select   E Edit   A Activate   R Remove   Q Quit".into(),
            format!("  {}", crop(&message, 74)),
        ]);
        term.draw(&frame).map_err(|e| e.to_string())?;
        if last_scan.elapsed() >= Duration::from_secs(2) { processes = model::running_paths(); last_scan = Instant::now(); }
        let Some(key) = read_key(Duration::from_millis(250)).map_err(|e| e.to_string())? else { continue; };
        match key {
            Key::Up => selected = selected.saturating_sub(1),
            Key::Down => selected = (selected + 1).min(total - 1),
            Key::Char('q' | 'Q') | Key::Esc => break,
            Key::Char('e' | 'E') if selected < config.entries.len() => {
                message = edit(&mut term, path, config, selected)?;
            }
            Key::Char('a' | 'A') if selected < config.entries.len() => {
                let entry = &config.entries[selected];
                message = if model::is_running(entry, &processes) { format!("Already running: {}", entry.name) }
                    else { match model::launch(entry) { Ok(()) => format!("Started: {}", entry.name), Err(e) => e } };
            }
            Key::Char('r' | 'R') | Key::Delete if selected < config.entries.len() => {
                let entry = &config.entries[selected];
                if term.input(&format!("Remove {}? Type Y and Enter: ", entry.name), "").map_err(|e| e.to_string())?.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("y")) {
                    let removed = config.entries.remove(selected);
                    match model::save(path, config) {
                        Ok(()) => message = format!("Removed: {}", removed.name),
                        Err(e) => { config.entries.insert(selected, removed); message = e; }
                    }
                } else { message = "Removal canceled".into(); }
            }
            Key::Enter if selected == config.entries.len() => { message = add(&mut term, path, config)?; }
            Key::Enter if selected == config.entries.len() + 1 => {
                let errors = model::launch_batch(config);
                batch_result(errors, Some(&mut term))?;
                break;
            }
            Key::Enter if selected == config.entries.len() + 2 => break,
            Key::Enter if selected < config.entries.len() => {
                message = format!("{}: E edit · A activate · R remove", config.entries[selected].name);
            }
            _ => {}
        }
        processes = model::running_paths();
        last_scan = Instant::now();
    }
    Ok(())
}

fn add(term: &mut Terminal, path: &Path, config: &mut Config) -> Result<String, String> {
    let Some(name) = term.input("New app name (Esc cancels): ", "").map_err(|e| e.to_string())? else { return Ok("Add canceled".into()); };
    if name.trim().is_empty() { return Ok("Name cannot be empty".into()); }
    let start = env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from).unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let Some(file) = pick_file(term, &start)? else { return Ok("Add canceled".into()); };
    let entry = match model::validated_entry(&name, &file, Mode::Individual) { Ok(e) => e, Err(e) => return Ok(e) };
    config.entries.push(entry);
    match model::save(path, config) {
        Ok(()) => Ok(format!("Added: {}", name.trim())),
        Err(e) => { config.entries.pop(); Ok(e) }
    }
}

fn edit(term: &mut Terminal, path: &Path, config: &mut Config, i: usize) -> Result<String, String> {
    let original = config.entries[i].clone();
    let Some(choice) = term.input("Edit [N]ame, [P]ath, or [M]ode (Esc cancels): ", "").map_err(|e| e.to_string())? else { return Ok("Edit canceled".into()); };
    let mut updated = original.clone();
    match choice.trim().to_ascii_lowercase().as_str() {
        "n" | "name" => {
            let Some(name) = term.input("Name: ", &original.name).map_err(|e| e.to_string())? else { return Ok("Edit canceled".into()); };
            updated.name = name;
        }
        "p" | "path" => {
            let start = original.path.parent().unwrap_or(Path::new("."));
            let Some(file) = pick_file(term, start)? else { return Ok("Edit canceled".into()); };
            updated.path = file;
        }
        "m" | "mode" => { updated.mode = if original.mode == Mode::Batch { Mode::Individual } else { Mode::Batch }; }
        _ => return Ok("Edit canceled".into()),
    }
    updated = match model::validated_entry(&updated.name, &updated.path, updated.mode) { Ok(e) => e, Err(e) => return Ok(e) };
    config.entries[i] = updated;
    match model::save(path, config) {
        Ok(()) => Ok(format!("Updated: {}", config.entries[i].name)),
        Err(e) => { config.entries[i] = original; Ok(e) }
    }
}

fn pick_file(term: &mut Terminal, start: &Path) -> Result<Option<PathBuf>, String> {
    let mut dir = start.to_path_buf();
    let mut selected = 0usize;
    loop {
        let mut items: Vec<_> = std::fs::read_dir(&dir).map_err(|e| format!("Cannot browse {}: {e}", dir.display()))?
            .flatten().map(|e| e.path()).filter(|p| p.is_dir() || p.is_file()).collect();
        items.sort_by(|a, b| b.is_dir().cmp(&a.is_dir()).then_with(|| a.file_name().cmp(&b.file_name())));
        selected = selected.min(items.len());
        let start_row = selected.saturating_sub(14);
        let mut frame = vec!["\x1b[1;91m  SELECT APPLICATION\x1b[0m".into(), format!("  {}", crop(&dir.display().to_string(), 74)), "".into()];
        frame.push(format!("{}  ../", if selected == 0 { "\x1b[95m▶\x1b[0m" } else { " " }));
        for (i, item) in items.iter().enumerate().skip(start_row).take(16) {
            let name = item.file_name().unwrap_or_default().to_string_lossy();
            frame.push(format!("{}  {}{}", if selected == i + 1 { "\x1b[95m▶\x1b[0m" } else { " " }, crop(&name, 70), if item.is_dir() { "/" } else { "" }));
        }
        frame.extend(["".into(), "  ↑↓ Navigate   Enter Open / Select   Backspace Parent   Esc Cancel".into()]);
        term.draw(&frame).map_err(|e| e.to_string())?;
        match read_key(Duration::from_secs(3600)).map_err(|e| e.to_string())? {
            Some(Key::Up) => selected = selected.saturating_sub(1),
            Some(Key::Down) => selected = (selected + 1).min(items.len()),
            Some(Key::Backspace) => { if let Some(parent) = dir.parent() { dir = parent.to_path_buf(); selected = 0; } }
            Some(Key::Esc) => { term.invalidate(); return Ok(None); }
            Some(Key::Enter) if selected == 0 => { if let Some(parent) = dir.parent() { dir = parent.to_path_buf(); selected = 0; } }
            Some(Key::Enter) => {
                let path = items[selected - 1].clone();
                if path.is_dir() { dir = path; selected = 0; }
                else { term.invalidate(); return Ok(Some(path)); }
            }
            _ => {}
        }
    }
}

fn batch_result(errors: Vec<String>, mut term: Option<&mut Terminal>) -> Result<(), String> {
    if !errors.is_empty() {
        if let Some(t) = term.as_deref_mut() {
            let mut first = 0usize;
            loop {
                let mut lines = vec![format!("\x1b[1;91m  BATCH ERRORS\x1b[0m  {}–{} of {}", first + 1, (first + 17).min(errors.len()), errors.len()), "".into()];
                lines.extend(errors.iter().skip(first).take(17).map(|e| format!("  {}", crop(e, 74))));
                lines.push("".into()); lines.push("  ↑↓ Scroll errors   Enter Quit".into());
                t.draw(&lines).map_err(|e| e.to_string())?;
                match read_key(Duration::from_secs(3600)).map_err(|e| e.to_string())? {
                    Some(Key::Up) => first = first.saturating_sub(1),
                    Some(Key::Down) => first = (first + 1).min(errors.len().saturating_sub(17)),
                    Some(Key::Enter) => break,
                    _ => {}
                }
            }
        } else {
            for error in &errors { eprintln!("{error}"); }
            print!("Press Enter to quit..."); io::stdout().flush().map_err(|e| e.to_string())?;
            let mut answer = String::new(); io::stdin().read_line(&mut answer).map_err(|e| e.to_string())?;
        }
        return Err(format!("{} application(s) failed to start", errors.len()));
    }
    for seconds in (1..=5).rev() {
        if let Some(t) = term.as_deref_mut() {
            t.draw(&["\x1b[1;92m  JOB DONE\x1b[0m".into(), "".into(), format!("  Auto-close in {seconds}...")]).map_err(|e| e.to_string())?;
            let _ = read_key(Duration::from_secs(1)).map_err(|e| e.to_string())?;
        } else {
            print!("\rJob Done. Auto-close in {seconds}... ");
            io::stdout().flush().map_err(|e| e.to_string())?;
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    if term.is_none() { println!(); }
    Ok(())
}
