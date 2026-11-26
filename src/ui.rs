use crate::{batch_result, crop, model::{self, Config, Entry, Mode}, terminal::{read_key, Key, Terminal}};
use std::{collections::HashSet, fs, path::{Path, PathBuf}, sync::mpsc::{self, Receiver, TryRecvError}, time::{Duration, Instant}};

const VISIBLE_ROWS: usize = 6;
const MENU_ROWS: usize = 5;

enum Outcome {
    Saved { result: Result<Config, String>, message: String },
    Action(Result<String, String>),
    Batch(Vec<String>),
}

fn spawn<F>(operation: F) -> Receiver<Outcome>
where F: FnOnce() -> Outcome + Send + 'static {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || { let _ = sender.send(operation()); });
    receiver
}

fn save_async(path: &Path, config: Config, message: String) -> Receiver<Outcome> {
    let path = path.to_path_buf();
    spawn(move || {
        let result = model::save(&path, &config).map(|_| config);
        Outcome::Saved { result, message }
    })
}

fn path_errors(config: &Config) -> Vec<Option<String>> {
    config.entries.iter().map(|entry| {
        model::check_path(&entry.path).err().map(|error| format!("{}: {error}", entry.name))
    }).collect()
}

fn marker(selected: bool, busy: bool) -> &'static str {
    if !selected { " " } else if busy { "▶" } else { "\x1b[95m▶\x1b[0m" }
}

fn row(entry: &Entry, selected: bool, running: bool, invalid: bool, busy: bool) -> String {
    let setting = if invalid { "Not Found".to_string() } else { entry.mode.to_string() };
    let status = if !invalid && running { if busy { "RUNNING" } else { "\x1b[92mRUNNING\x1b[0m" } }
        else if busy { "OFF" } else { "\x1b[91mOFF\x1b[0m" };
    format!("{} {:<30}  │  {:<12}  │  {status}", marker(selected, busy), crop(&entry.name, 30), setting)
}

fn footer(selected: bool, busy: bool, label: &str) -> String {
    format!("{} {label}", marker(selected, busy))
}

fn frame(config: &Config, selected: usize, processes: &std::collections::HashSet<String>, errors: &[Option<String>], message: &str, busy: bool) -> Vec<String> {
    let mut lines = vec![
        if busy { "  ⚡ REDSTART  Application launcher".into() } else { "\x1b[1;91m  ⚡ REDSTART\x1b[0m  Application launcher".into() },
        String::new(),
        format!("  {:<30}  │  {:<12}  │  STATUS", "NAME", "SETTING"),
        format!("  {}──┼──{}──┼──{}", "─".repeat(30), "─".repeat(12), "─".repeat(10)),
    ];
    let start = selected.saturating_sub(VISIBLE_ROWS - 1).min(config.entries.len().saturating_sub(VISIBLE_ROWS));
    if start > 0 { lines.push(format!("  ↑ {start} earlier rows")); }
    for (i, entry) in config.entries.iter().enumerate().skip(start).take(VISIBLE_ROWS) {
        lines.push(row(entry, selected == i, model::is_running(entry, processes), errors[i].is_some(), busy));
    }
    if start + VISIBLE_ROWS < config.entries.len() { lines.push(format!("  ↓ {} more rows", config.entries.len() - start - VISIBLE_ROWS)); }
    lines.extend([
        String::new(),
        footer(selected == config.entries.len(), busy, "+ Add a new app"),
        footer(selected == config.entries.len() + 1, busy, "Relocate an app"),
        footer(selected == config.entries.len() + 2, busy, "Remove an app"),
        footer(selected == config.entries.len() + 3, busy, "▶ Run Batch and Close"),
        footer(selected == config.entries.len() + 4, busy, "Close"),
        String::new(),
        "  Enter Start/Stop  Tab Mode  F2 Rename".into(),
        "  ↑↓ Navigate  Esc Close".into(),
    ]);
    if busy {
        for line in &mut lines { *line = format!("\x1b[2m{line}\x1b[0m"); }
        lines.push("  Working... Esc: force quit".into());
    } else {
        let detail = errors.get(selected).and_then(|error| error.as_deref()).unwrap_or(message);
        lines.push(format!("  {}", crop(detail, 74)));
    }
    lines
}

pub fn run(path: &Path, config: &mut Config) -> Result<(), String> {
    let mut term = Terminal::new().map_err(|e| e.to_string())?;
    let mut selected = 0usize;
    let mut errors = path_errors(config);
    let invalid_count = errors.iter().filter(|error| error.is_some()).count();
    let mut message = if invalid_count > 0 { format!("{invalid_count} invalid app path(s). Select a row for details.") }
        else if config.entries.is_empty() { "Ready. Add an app to get started.".into() } else { "Ready.".into() };
    let mut processes = model::running_paths();
    let mut last_scan = Instant::now();
    let mut pending: Option<Receiver<Outcome>> = None;
    let mut selection_after_save: Option<usize> = None;
    loop {
        selected = selected.min(config.entries.len() + MENU_ROWS - 1);
        term.draw(&frame(config, selected, &processes, &errors, &message, pending.is_some())).map_err(|e| e.to_string())?;
        if let Some(receiver) = pending.as_ref() {
            match receiver.try_recv() {
                Ok(Outcome::Saved { result, message: success }) => {
                    pending = None;
                    match result {
                        Ok(saved) => {
                            *config = saved;
                            errors = path_errors(config);
                            message = success;
                            if let Some(next) = selection_after_save.take() { selected = next; }
                        }
                        Err(error) => { message = error; selection_after_save = None; }
                    }
                    processes = model::running_paths();
                    last_scan = Instant::now();
                    continue;
                }
                Ok(Outcome::Action(result)) => {
                    pending = None;
                    message = result.unwrap_or_else(|error| error);
                    processes = model::running_paths();
                    last_scan = Instant::now();
                    continue;
                }
                Ok(Outcome::Batch(batch_errors)) => {
                    batch_result(batch_errors, Some(&mut term))?;
                    break;
                }
                Err(TryRecvError::Disconnected) => { pending = None; message = "Operation failed unexpectedly".into(); continue; }
                Err(TryRecvError::Empty) => {}
            }
            if read_key(Duration::from_millis(50)).map_err(|e| e.to_string())? == Some(Key::Esc) { break; }
            continue;
        }
        if last_scan.elapsed() >= Duration::from_secs(2) {
            processes = model::running_paths();
            last_scan = Instant::now();
        }
        let Some(key) = read_key(Duration::from_millis(250)).map_err(|e| e.to_string())? else { continue; };
        match key {
            Key::Up => selected = selected.saturating_sub(1),
            Key::Down => selected = (selected + 1).min(config.entries.len() + MENU_ROWS - 1),
            Key::Esc => break,
            Key::Enter | Key::Tab if selected < config.entries.len() && errors[selected].is_some() => {
                match relocate(&mut term, config, selected, path) {
                    Ok(Some(receiver)) => pending = Some(receiver),
                    Ok(None) => message = "Location change canceled".into(),
                    Err(error) => message = error,
                }
            }
            Key::Enter if selected < config.entries.len() => {
                let entry = config.entries[selected].clone();
                let running = model::is_running(&entry, &processes);
                pending = Some(spawn(move || Outcome::Action(if running {
                    model::stop(&entry).map(|_| format!("Stopped: {}", entry.name))
                } else {
                    model::launch(&entry).map(|_| format!("Started: {}", entry.name))
                })));
            }
            Key::Tab if selected < config.entries.len() => {
                let mut updated = config.clone();
                let entry = &mut updated.entries[selected];
                entry.mode = if entry.mode == Mode::Batch { Mode::Individual } else { Mode::Batch };
                let success = format!("{}: {}", entry.name, entry.mode);
                pending = Some(save_async(path, updated, success));
            }
            Key::F2 if selected < config.entries.len() => {
                match rename(&mut term, &config.entries[selected]) {
                    Ok(Some(updated)) => {
                        let mut next = config.clone();
                        let success = format!("Renamed: {}", updated.name);
                        next.entries[selected] = updated;
                        pending = Some(save_async(path, next, success));
                    }
                    Ok(None) => message = "Rename canceled".into(),
                    Err(error) => message = error,
                }
            }
            Key::Enter if selected == config.entries.len() => {
                match add(&mut term) {
                    Ok(Some(entry)) => {
                        let mut next = config.clone();
                        let success = format!("Added: {}", entry.name);
                        next.entries.push(entry);
                        pending = Some(save_async(path, next, success));
                    }
                    Ok(None) => message = "Add canceled".into(),
                    Err(error) => message = error,
                }
            }
            Key::Enter if selected == config.entries.len() + 1 => {
                if config.entries.is_empty() { message = "No apps configured".into(); continue; }
                match choose_app(&mut term, config, &errors, &processes, "RELOCATE APP") {
                    Ok(Some(index)) => match relocate(&mut term, config, index, path) {
                        Ok(Some(receiver)) => pending = Some(receiver),
                        Ok(None) => message = "Location change canceled".into(),
                        Err(error) => message = error,
                    },
                    Ok(None) => message = "Location change canceled".into(),
                    Err(error) => message = error,
                }
            }
            Key::Enter if selected == config.entries.len() + 2 => {
                if config.entries.is_empty() { message = "No apps configured".into(); continue; }
                match choose_app(&mut term, config, &errors, &processes, "REMOVE APP") {
                    Ok(Some(index)) => {
                        let entry = &config.entries[index];
                        let confirmed = confirm_remove(&mut term, &entry.name)?;
                        if confirmed {
                            let mut next = config.clone();
                            let removed = next.entries.remove(index);
                            selection_after_save = Some(next.entries.len() + 2);
                            pending = Some(save_async(path, next, format!("Removed: {}", removed.name)));
                        } else { message = "Removal canceled".into(); }
                    }
                    Ok(None) => message = "Removal canceled".into(),
                    Err(error) => message = error,
                }
            }
            Key::Enter if selected == config.entries.len() + 3 => {
                let batch = config.clone();
                pending = Some(spawn(move || Outcome::Batch(model::launch_batch(&batch))));
            }
            Key::Enter if selected == config.entries.len() + 4 => break,
            _ => {}
        }
    }
    Ok(())
}

fn add(term: &mut Terminal) -> Result<Option<Entry>, String> {
    let Some(name) = term.input("New app name: ", "").map_err(|e| e.to_string())? else { return Ok(None); };
    if name.trim().is_empty() { return Err("Name cannot be empty".into()); }
    let Some(file) = pick_file(term)? else { return Ok(None); };
    model::validated_entry(&name, &file, Mode::Individual).map(Some)
}

fn rename(term: &mut Terminal, entry: &Entry) -> Result<Option<Entry>, String> {
    let Some(name) = term.input("Rename: ", &entry.name).map_err(|e| e.to_string())? else { return Ok(None); };
    let name = name.trim();
    if name.is_empty() { return Err("Name cannot be empty".into()); }
    let mut updated = entry.clone();
    updated.name = name.to_string();
    Ok(Some(updated))
}

fn change_location(term: &mut Terminal, entry: &Entry) -> Result<Option<Entry>, String> {
    let Some(file) = pick_file(term)? else { return Ok(None); };
    model::validated_entry(&entry.name, &file, entry.mode).map(Some)
}

fn relocate(term: &mut Terminal, config: &Config, index: usize, path: &Path) -> Result<Option<Receiver<Outcome>>, String> {
    let Some(updated) = change_location(term, &config.entries[index])? else { return Ok(None); };
    let mut next = config.clone();
    let success = format!("Location updated: {}", updated.name);
    next.entries[index] = updated;
    Ok(Some(save_async(path, next, success)))
}

fn choose_app(term: &mut Terminal, config: &Config, errors: &[Option<String>], processes: &HashSet<String>, title: &str) -> Result<Option<usize>, String> {
    let mut selected = 0usize;
    loop {
        let start = selected.saturating_sub(11);
        let mut lines = vec![format!("\x1b[1;91m  {title}\x1b[0m"), String::new(),
            format!("  {:<30}  │  {:<12}  │  STATUS", "NAME", "SETTING"),
            format!("  {}──┼──{}──┼──{}", "─".repeat(30), "─".repeat(12), "─".repeat(10))];
        if start > 0 { lines.push(format!("  ↑ {start} earlier rows")); }
        for (index, entry) in config.entries.iter().enumerate().skip(start).take(12) {
            lines.push(row(entry, selected == index, model::is_running(entry, processes), errors[index].is_some(), false));
        }
        if start + 12 < config.entries.len() { lines.push(format!("  ↓ {} more rows", config.entries.len() - start - 12)); }
        lines.extend([String::new(), "  ↑↓ Navigate  Enter Select  Esc Cancel".into()]);
        term.draw(&lines).map_err(|e| e.to_string())?;
        match read_key(Duration::from_secs(3600)).map_err(|e| e.to_string())? {
            Some(Key::Up) => selected = selected.saturating_sub(1),
            Some(Key::Down) => selected = (selected + 1).min(config.entries.len() - 1),
            Some(Key::Enter) => { term.invalidate(); return Ok(Some(selected)); }
            Some(Key::Esc) => { term.invalidate(); return Ok(None); }
            _ => {}
        }
    }
}

fn confirm_remove(term: &mut Terminal, name: &str) -> Result<bool, String> {
    let mut yes = false;
    loop {
        let no_option = if yes { " No " } else { "\x1b[7m No \x1b[0m" };
        let yes_option = if yes { "\x1b[7m Yes \x1b[0m" } else { " Yes " };
        let lines = vec![
            "\x1b[1;91m  REMOVE APP\x1b[0m".into(),
            String::new(),
            format!("  Remove {}?", crop(name, 65)),
            String::new(),
            format!("  {no_option}    {yes_option}"),
            String::new(),
            "  ←→ Choose  Enter Confirm  Esc Cancel".into(),
        ];
        term.draw(&lines).map_err(|e| e.to_string())?;
        match read_key(Duration::from_secs(3600)).map_err(|e| e.to_string())? {
            Some(Key::Left) => yes = false,
            Some(Key::Right) => yes = true,
            Some(Key::Enter) => { term.invalidate(); return Ok(yes); }
            Some(Key::Esc) => { term.invalidate(); return Ok(false); }
            _ => {}
        }
    }
}

fn system_root() -> PathBuf {
    #[cfg(windows)] {
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        PathBuf::from(format!("{}\\", drive.trim_end_matches(['\\', '/'])))
    }
    #[cfg(unix)] { PathBuf::from("/") }
}

fn pick_file(term: &mut Terminal) -> Result<Option<PathBuf>, String> {
    let mut dir = system_root();
    let mut selected = 0usize;
    loop {
        let mut items: Vec<_> = fs::read_dir(&dir).map_err(|e| format!("Cannot browse {}: {e}", dir.display()))?
            .flatten().map(|item| item.path()).filter(|path| path.is_dir() || path.is_file()).collect();
        items.sort_by(|a, b| b.is_dir().cmp(&a.is_dir()).then_with(|| a.file_name().cmp(&b.file_name())));
        selected = selected.min(items.len());
        let start_row = selected.saturating_sub(14);
        let mut lines = vec!["\x1b[1;91m  SELECT APPLICATION\x1b[0m".into(), format!("  {}", crop(&dir.display().to_string(), 74)), String::new()];
        lines.push(format!("{}  ../", marker(selected == 0, false)));
        for (i, item) in items.iter().enumerate().skip(start_row).take(16) {
            let name = item.file_name().unwrap_or_default().to_string_lossy();
            lines.push(format!("{}  {}{}", marker(selected == i + 1, false), crop(&name, 70), if item.is_dir() { "/" } else { "" }));
        }
        lines.extend([String::new(), "  ↑↓ Navigate  Enter Open/Select  Backspace Parent  Esc Cancel".into()]);
        term.draw(&lines).map_err(|e| e.to_string())?;
        match read_key(Duration::from_secs(3600)).map_err(|e| e.to_string())? {
            Some(Key::Up) => selected = selected.saturating_sub(1),
            Some(Key::Down) => selected = (selected + 1).min(items.len()),
            Some(Key::Backspace) | Some(Key::Enter) if selected == 0 => {
                if let Some(parent) = dir.parent() { dir = parent.to_path_buf(); selected = 0; }
            }
            Some(Key::Backspace) => { if let Some(parent) = dir.parent() { dir = parent.to_path_buf(); selected = 0; } }
            Some(Key::Esc) => { term.invalidate(); return Ok(None); }
            Some(Key::Enter) => {
                let path = items[selected - 1].clone();
                if path.is_dir() { dir = path; selected = 0; }
                else { term.invalidate(); return Ok(Some(path)); }
            }
            #[cfg(windows)] Some(Key::Char(letter)) if dir.parent().is_none() && letter.is_ascii_alphabetic() => {
                let drive = PathBuf::from(format!("{}:\\", letter.to_ascii_uppercase()));
                if drive.exists() { dir = drive; selected = 0; }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_align_for_both_modes_and_invalid_paths() {
        let config = Config { entries: vec![
            Entry { name: "Short".into(), path: PathBuf::from("missing-one"), mode: Mode::Individual },
            Entry { name: "Longer application name".into(), path: PathBuf::from("missing-two"), mode: Mode::Batch },
        ] };
        let errors = vec![Some("Short: missing path".into()), None];
        let lines = frame(&config, 2, &Default::default(), &errors, "Ready", false);
        let columns = |line: &str| line.chars().enumerate().filter_map(|(index, c)| (c == '│').then_some(index)).collect::<Vec<_>>();
        assert_eq!(columns(&lines[2]), vec![34, 51]);
        assert_eq!(columns(&lines[2]), columns(&lines[4]));
        assert_eq!(columns(&lines[2]), columns(&lines[5]));
        assert!(lines[4].contains("Not Found"));
        assert!(!lines[4].contains("INDIVIDUAL"));
        let relocate = lines.iter().position(|line| line.contains("Relocate an app")).unwrap();
        let remove = lines.iter().position(|line| line.contains("Remove an app")).unwrap();
        let batch = lines.iter().position(|line| line.contains("▶ Run Batch and Close")).unwrap();
        let close = lines.iter().position(|line| line.contains("Close") && !line.contains("Batch") && !line.contains("Esc")).unwrap();
        assert!(relocate < remove && remove < batch && batch < close);
        let selected = frame(&config, 0, &Default::default(), &errors, "Ready", false);
        assert!(selected.last().unwrap().contains("Short: missing path"));
        let working = frame(&config, 0, &Default::default(), &errors, "Ready", true);
        assert!(working[..working.len() - 1].iter().all(|line| line.starts_with("\x1b[2m")));
        assert!(working.last().unwrap().contains("Working..."));
    }
}
