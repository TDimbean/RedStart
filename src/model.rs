use serde::{Deserialize, Serialize};
use std::{collections::{HashMap, HashSet}, fs, io, path::{Path, PathBuf}, process::{Command, Stdio}};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode { Individual, Batch }

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self { Self::Individual => "INDIVIDUAL", Self::Batch => "BATCH" })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Entry { pub name: String, pub path: PathBuf, pub mode: Mode }

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Config { pub entries: Vec<Entry> }

pub fn config_path() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("REDSTART_CONFIG") { return Ok(PathBuf::from(path)); }
    Ok(std::env::current_exe()?.parent().ok_or_else(|| io::Error::other("executable has no parent directory"))?.join("redstart.json"))
}

pub fn load(path: &Path) -> Result<Config, String> {
    #[cfg(windows)] {
        let backup = path.with_extension("json.bak");
        if !path.exists() && backup.exists() {
            fs::rename(&backup, path).map_err(|e| format!("Cannot restore {}: {e}", path.display()))?;
        }
    }
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("Cannot read {}: {e}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let config = Config::default();
            save(path, &config)?;
            Ok(config)
        }
        Err(e) => Err(format!("Cannot read {}: {e}", path.display())),
    }
}

pub fn save(path: &Path, config: &Config) -> Result<(), String> {
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(config).map_err(|e| e.to_string())?;
    fs::write(&temp, bytes).map_err(|e| format!("Cannot write {}: {e}", temp.display()))?;
    #[cfg(unix)] { fs::rename(&temp, path).map_err(|e| format!("Cannot replace {}: {e}", path.display())) }
    #[cfg(windows)] {
        let backup = path.with_extension("json.bak");
        if path.exists() {
            fs::copy(path, &backup).map_err(|e| format!("Cannot back up {}: {e}", path.display()))?;
            fs::remove_file(path).map_err(|e| format!("Cannot replace {}: {e}", path.display()))?;
        }
        if let Err(e) = fs::rename(&temp, path) {
            if backup.exists() { let _ = fs::rename(&backup, path); }
            return Err(format!("Cannot replace {}: {e}", path.display()));
        }
        let _ = fs::remove_file(backup);
        Ok(())
    }
}

pub fn validated_entry(name: &str, path: &Path, mode: Mode) -> Result<Entry, String> {
    let name = name.trim();
    if name.is_empty() { return Err("Name cannot be empty".into()); }
    let path = fs::canonicalize(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    check_path(&path)?;
    Ok(Entry { name: name.to_string(), path, mode })
}

pub fn check_path(path: &Path) -> Result<(), String> {
    let path = fs::canonicalize(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    if !path.is_file() { return Err(format!("{} is not a file", path.display())); }
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
        if metadata.permissions().mode() & 0o111 == 0 { return Err(format!("{} is not executable", path.display())); }
    }
    #[cfg(windows)] {
        let ext = path.extension().and_then(|x| x.to_str()).unwrap_or_default().to_ascii_lowercase();
        if ext != "exe" && ext != "com" { return Err("Select a .exe or .com application".into()); }
    }
    Ok(())
}

pub fn launch(entry: &Entry) -> Result<(), String> {
    check_path(&entry.path).map_err(|e| format!("{}: {e}", entry.name))?;
    let parent = entry.path.parent().ok_or("Application path has no parent")?;
    Command::new(&entry.path)
        .current_dir(parent)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", entry.name))
}

fn key(path: &Path) -> String {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = path.to_string_lossy().into_owned();
    if cfg!(windows) { text.to_ascii_lowercase() } else { text }
}

#[cfg(unix)]
fn processes() -> HashMap<String, Vec<u32>> {
    let mut paths: HashMap<String, Vec<u32>> = HashMap::new();
    if let Ok(processes) = fs::read_dir("/proc") {
        for process in processes.flatten() {
            let Ok(pid) = process.file_name().to_string_lossy().parse::<u32>() else { continue; };
            if let Ok(path) = fs::read_link(process.path().join("exe")) { paths.entry(key(&path)).or_default().push(pid); }
        }
    }
    paths
}

#[cfg(windows)]
fn processes() -> HashMap<String, Vec<u32>> {
    use windows_sys::Win32::{Foundation::{CloseHandle, INVALID_HANDLE_VALUE}, System::{Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS}, Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION}}};
    let mut paths: HashMap<String, Vec<u32>> = HashMap::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE { return paths; }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
                if !handle.is_null() {
                    let mut buffer = [0u16; 32768];
                    let mut length = buffer.len() as u32;
                    if QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) != 0 {
                        paths.entry(key(&PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize])))).or_default().push(entry.th32ProcessID);
                    }
                    CloseHandle(handle);
                }
                if Process32NextW(snapshot, &mut entry) == 0 { break; }
            }
        }
        CloseHandle(snapshot);
    }
    paths
}

pub fn running_paths() -> HashSet<String> { processes().into_keys().collect() }

pub fn is_running(entry: &Entry, processes: &HashSet<String>) -> bool { processes.contains(&key(&entry.path)) }

pub fn stop(entry: &Entry) -> Result<(), String> {
    let target = key(&entry.path);
    let ids = processes().remove(&target).unwrap_or_default();
    if ids.is_empty() { return Err(format!("{} is not running", entry.name)); }
    if ids.contains(&std::process::id()) { return Err("RedStart cannot stop itself".into()); }
    #[cfg(windows)] {
        use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE}};
        let mut errors = Vec::new();
        for pid in ids {
            unsafe {
                let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
                if handle.is_null() { errors.push(format!("PID {pid}: {}", io::Error::last_os_error())); continue; }
                if TerminateProcess(handle, 1) == 0 { errors.push(format!("PID {pid}: {}", io::Error::last_os_error())); }
                CloseHandle(handle);
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(format!("{}: {}", entry.name, errors.join("; "))) }
    }
    #[cfg(unix)] {
        let mut errors = Vec::new();
        for &pid in &ids {
            if unsafe { libc::kill(pid as i32, libc::SIGTERM) } != 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) { errors.push(format!("PID {pid}: {error}")); }
            }
        }
        if !errors.is_empty() { return Err(format!("{}: {}", entry.name, errors.join("; "))); }
        for _ in 0..20 {
            let current = processes().remove(&target).unwrap_or_default();
            if !ids.iter().any(|id| current.contains(id)) { return Ok(()); }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let current = processes().remove(&target).unwrap_or_default();
        for pid in ids.into_iter().filter(|id| current.contains(id)) {
            if unsafe { libc::kill(pid as i32, libc::SIGKILL) } != 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) { errors.push(format!("PID {pid}: {error}")); }
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(format!("{}: {}", entry.name, errors.join("; "))) }
    }
}

pub fn launch_batch(config: &Config) -> Vec<String> {
    let mut errors = Vec::new();
    let mut processes = running_paths();
    for entry in config.entries.iter().filter(|e| e.mode == Mode::Batch) {
        if let Err(error) = check_path(&entry.path) { errors.push(format!("{}: {error}", entry.name)); continue; }
        if !is_running(entry, &processes) {
            match launch(entry) {
                Ok(()) => { processes.insert(key(&entry.path)); }
                Err(error) => errors.push(error),
            }
        }
    }
    errors
}
