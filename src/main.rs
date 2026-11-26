mod model;
mod terminal;
mod ui;

use model::{Config, Mode};
use std::{env, io::{self, IsTerminal, Write}, path::{Path, PathBuf}, time::{Duration, Instant}};
use terminal::{Key, Terminal, read_key};

fn main() {
    if let Err(error) = run() { eprintln!("Error: {error}"); std::process::exit(1); }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if matches!(args.first().map(String::as_str), Some("help" | "--help" | "-h")) { help(); return Ok(()); }
    let config_path = model::config_path().map_err(|e| e.to_string())?;
    let mut config = model::load(&config_path)?;
    if args.is_empty() { return ui::run(&config_path, &mut config); }
    cli(&args, &config_path, &mut config)
}

fn help() {
    println!("RedStart\n\nUSAGE\n  redstart [command]\n\nCOMMANDS\n  list\n  add <name> <path> [--batch]\n  edit <number> [--name <name>] [--path <path>] [--mode individual|batch]\n  remove <number> [--yes]\n  activate <number>\n  stop <number>\n  batch\n  help\n\nNo command opens the keyboard interface. Data is saved in redstart.json next to the executable.");
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
                let found = model::check_path(&entry.path).is_ok();
                let setting = if found { entry.mode.to_string() } else { "Not Found".into() };
                let status = if found && model::is_running(entry, &processes) { "RUNNING" } else { "OFF" };
                println!("{:>3}  {:<28} {:<11} {:<7} {}", i + 1, entry.name, setting, status, entry.path.display());
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
        "stop" => {
            if args.len() != 2 { return Err("Usage: stop <number>".into()); }
            let i = index(&args[1], config.entries.len())?;
            model::stop(&config.entries[i])?;
            println!("Stopped: {}", config.entries[i].name);
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

pub(crate) fn crop(value: &str, max: usize) -> String {
    if value.chars().count() <= max { value.to_string() }
    else { format!("{}…", value.chars().take(max - 1).collect::<String>()) }
}

pub(crate) fn batch_result(errors: Vec<String>, mut term: Option<&mut Terminal>) -> Result<(), String> {
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
    let _inline = if term.is_none() && io::stdin().is_terminal() { Terminal::new_inline().ok() } else { None };
    for seconds in (1..=5).rev() {
        if let Some(t) = term.as_deref_mut() {
            t.draw(&["\x1b[1;92m  JOB DONE\x1b[0m".into(), "".into(), format!("  Auto-close in {seconds}...")]).map_err(|e| e.to_string())?;
            let deadline = Instant::now() + Duration::from_secs(1);
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() { break; }
                if read_key(remaining).map_err(|e| e.to_string())?.is_some() { return Ok(()); }
            }
        } else {
            print!("\rJob Done. Auto-close in {seconds}... ");
            io::stdout().flush().map_err(|e| e.to_string())?;
            if _inline.is_some() {
                if read_key(Duration::from_secs(1)).map_err(|e| e.to_string())?.is_some() { println!(); return Ok(()); }
            } else { std::thread::sleep(Duration::from_secs(1)); }
        }
    }
    if term.is_none() { println!(); }
    Ok(())
}
