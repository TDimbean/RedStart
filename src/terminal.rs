use std::{io::{self, Write}, time::Duration};

#[derive(Debug, PartialEq, Eq)]
pub enum Key { Up, Down, Left, Right, Enter, Esc, Backspace, Delete, Char(char) }

pub struct Terminal {
    old_lines: Vec<String>,
    #[cfg(unix)] old_mode: libc::termios,
    #[cfg(windows)] input_mode: u32,
    #[cfg(windows)] output_mode: u32,
}

impl Terminal {
    pub fn new() -> io::Result<Self> {
        #[cfg(unix)] let old_mode = unsafe {
            let mut old: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut old) != 0 { return Err(io::Error::last_os_error()); }
            let mut raw = old;
            libc::cfmakeraw(&mut raw);
            if libc::tcsetattr(0, libc::TCSANOW, &raw) != 0 { return Err(io::Error::last_os_error()); }
            old
        };
        #[cfg(windows)] let (input_mode, output_mode) = unsafe {
            use windows_sys::Win32::System::Console::*;
            let input = GetStdHandle(STD_INPUT_HANDLE);
            let output = GetStdHandle(STD_OUTPUT_HANDLE);
            let mut old_input = 0;
            let mut old_output = 0;
            if GetConsoleMode(input, &mut old_input) == 0 || GetConsoleMode(output, &mut old_output) == 0 { return Err(io::Error::other("Run this app in a terminal")); }
            let raw_input = old_input & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT | ENABLE_VIRTUAL_TERMINAL_INPUT);
            if SetConsoleMode(input, raw_input) == 0 { return Err(io::Error::last_os_error()); }
            if SetConsoleMode(output, old_output | ENABLE_VIRTUAL_TERMINAL_PROCESSING) == 0 {
                let error = io::Error::last_os_error();
                SetConsoleMode(input, old_input);
                return Err(error);
            }
            (old_input, old_output)
        };
        print!("\x1b[?1049h\x1b[?25l\x1b[2J\x1b[H");
        io::stdout().flush()?;
        Ok(Self { old_lines: Vec::new(), #[cfg(unix)] old_mode, #[cfg(windows)] input_mode, #[cfg(windows)] output_mode })
    }

    pub fn draw(&mut self, lines: &[String]) -> io::Result<()> {
        let mut out = io::stdout().lock();
        for row in 0..self.old_lines.len().max(lines.len()) {
            let line = lines.get(row).map(String::as_str).unwrap_or("");
            if self.old_lines.get(row).map(String::as_str) != Some(line) {
                write!(out, "\x1b[{};1H\x1b[2K{}", row + 1, line)?;
            }
        }
        out.flush()?;
        self.old_lines = lines.to_vec();
        Ok(())
    }

    pub fn invalidate(&mut self) {
        self.old_lines.clear();
        print!("\x1b[2J\x1b[H");
        let _ = io::stdout().flush();
    }

    pub fn input(&mut self, label: &str, initial: &str) -> io::Result<Option<String>> {
        let mut value = initial.to_string();
        let row = self.old_lines.len() + 2;
        loop {
            print!("\x1b[{};1H\x1b[2K{}{}\x1b[?25h", row, label, value);
            io::stdout().flush()?;
            match read_key(Duration::from_secs(3600))? {
                Some(Key::Enter) => { print!("\x1b[?25l"); self.invalidate(); return Ok(Some(value)); }
                Some(Key::Esc) => { print!("\x1b[?25l"); self.invalidate(); return Ok(None); }
                Some(Key::Backspace) => { value.pop(); }
                Some(Key::Char(c)) if !c.is_control() => value.push(c),
                _ => {}
            }
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        #[cfg(unix)] unsafe { libc::tcsetattr(0, libc::TCSANOW, &self.old_mode); }
        #[cfg(windows)] unsafe {
            use windows_sys::Win32::System::Console::*;
            SetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), self.input_mode);
            SetConsoleMode(GetStdHandle(STD_OUTPUT_HANDLE), self.output_mode);
        }
        print!("\x1b[?25h\x1b[?1049l");
        let _ = io::stdout().flush();
    }
}

#[cfg(unix)]
fn byte(timeout: Duration) -> io::Result<Option<u8>> {
    unsafe {
        let mut pfd = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
        let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        let result = libc::poll(&mut pfd, 1, ms);
        if result < 0 { return Err(io::Error::last_os_error()); }
        if result == 0 { return Ok(None); }
    }
    let mut b = [0];
    let count = unsafe { libc::read(0, b.as_mut_ptr().cast(), 1) };
    if count < 0 { return Err(io::Error::last_os_error()); }
    if count == 0 { return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "terminal input closed")); }
    Ok(Some(b[0]))
}

#[cfg(unix)]
pub fn read_key(timeout: Duration) -> io::Result<Option<Key>> {
    let Some(first) = byte(timeout)? else { return Ok(None); };
    Ok(Some(match first {
        b'\r' | b'\n' => Key::Enter,
        8 | 127 => Key::Backspace,
        3 => Key::Esc,
        27 => {
            if byte(Duration::from_millis(30))? != Some(b'[') { return Ok(Some(Key::Esc)); }
            match byte(Duration::from_millis(30))? {
                Some(b'A') => Key::Up, Some(b'B') => Key::Down,
                Some(b'C') => Key::Right, Some(b'D') => Key::Left,
                Some(b'3') => { let _ = byte(Duration::from_millis(30))?; Key::Delete },
                _ => Key::Esc,
            }
        }
        b if b < 128 => Key::Char(b as char),
        b => {
            let size = if b & 0xe0 == 0xc0 { 2 } else if b & 0xf0 == 0xe0 { 3 } else if b & 0xf8 == 0xf0 { 4 } else { return Ok(None) };
            let mut bytes = vec![b];
            for _ in 1..size { if let Some(next) = byte(Duration::from_millis(50))? { bytes.push(next); } }
            let text = std::str::from_utf8(&bytes).ok();
            match text.and_then(|s| s.chars().next()) { Some(c) => Key::Char(c), None => return Ok(None) }
        }
    }))
}

#[cfg(windows)]
pub fn read_key(timeout: Duration) -> io::Result<Option<Key>> {
    use windows_sys::Win32::{Foundation::WAIT_OBJECT_0, System::{Console::{GetStdHandle, INPUT_RECORD, KEY_EVENT, ReadConsoleInputW, STD_INPUT_HANDLE}, Threading::WaitForSingleObject}};
    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let ms = left.as_millis().min(u32::MAX as u128) as u32;
        if unsafe { WaitForSingleObject(handle, ms) } != WAIT_OBJECT_0 { return Ok(None); }
        let mut record = INPUT_RECORD::default();
        let mut count = 0;
        if unsafe { ReadConsoleInputW(handle, &mut record, 1, &mut count) } == 0 { return Err(io::Error::last_os_error()); }
        if count == 0 || record.EventType != KEY_EVENT as u16 { continue; }
        let event = unsafe { record.Event.KeyEvent };
        if event.bKeyDown == 0 { continue; }
        let key = match event.wVirtualKeyCode {
            0x26 => Some(Key::Up), 0x28 => Some(Key::Down),
            0x25 => Some(Key::Left), 0x27 => Some(Key::Right),
            0x0d => Some(Key::Enter), 0x1b => Some(Key::Esc),
            0x08 => Some(Key::Backspace), 0x2e => Some(Key::Delete),
            _ => {
                let code = unsafe { event.uChar.UnicodeChar } as u32;
                if code == 3 { Some(Key::Esc) } else { char::from_u32(code).filter(|c| !c.is_control()).map(Key::Char) }
            }
        };
        if key.is_some() { return Ok(key); }
    }
}
