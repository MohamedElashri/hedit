//! A Ghostty VT instance on the UI thread, with bounded PTY IO on workers.
//! No shell is created until the user explicitly opens the terminal.
use libghostty_vt::{
    key, paste,
    render::{CellIterator, RowIterator},
    screen::CellWide,
    style::{RgbColor, Style},
    terminal::ScrollViewport,
    RenderState, Terminal, TerminalOptions,
};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
};

const IO_CHUNK: usize = 8192;
const QUEUE_SIZE: usize = 32;
pub const PASTE_LIMIT: usize = 256 * 1024;

pub struct Cell {
    pub text: String,
    pub foreground: RgbColor,
    pub background: RgbColor,
    pub style: Style,
    pub wide: bool,
    pub selected: bool,
}

pub struct Grid {
    pub rows: Vec<Vec<Cell>>,
    /// Whether each row continues the preceding soft-wrapped row.
    pub continuations: Vec<bool>,
    pub background: RgbColor,
    pub cursor: Option<(u16, u16)>,
}

enum Output {
    Bytes(Vec<u8>),
    Error(String),
}

pub struct Session {
    pub vt: Terminal<'static, 'static>,
    render: RenderState<'static>,
    row_iter: RowIterator<'static>,
    cell_iter: CellIterator<'static>,
    encoder: key::Encoder<'static>,
    master: Box<dyn MasterPty + Send>,
    child: Option<Box<dyn Child + Send + Sync>>,
    input: SyncSender<Vec<u8>>,
    output: Receiver<Output>,
    pub directory: PathBuf,
    pub status: String,
    pub running: bool,
    pub output_generation: u64,
    size: Rc<std::cell::Cell<(u16, u16, u32, u32)>>,
}

impl Session {
    pub fn start(
        directory: &Path,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self, String> {
        Self::spawn(directory, CommandBuilder::new_default_prog(), wake)
    }

    fn spawn(
        directory: &Path,
        mut command: CommandBuilder,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let mut vt = Terminal::new(TerminalOptions {
            cols: 80,
            rows: 24,
            max_scrollback: 10_000,
        })
        .map_err(|e| e.to_string())?;
        // Allocate fallible renderer state before starting a process.
        let render = RenderState::new().map_err(|e| e.to_string())?;
        let row_iter = RowIterator::new().map_err(|e| e.to_string())?;
        let cell_iter = CellIterator::new().map_err(|e| e.to_string())?;
        let encoder = key::Encoder::new().map_err(|e| e.to_string())?;
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        command.cwd(directory);
        // Use a widely installed terminfo entry; Ghostty handles its VT sequences.
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "hedit");
        command.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
        let (input, writes) = mpsc::sync_channel::<Vec<u8>>(QUEUE_SIZE);
        let (output, reads) = mpsc::sync_channel(QUEUE_SIZE);
        let reply = input.clone();
        let reply_errors = output.clone();
        vt.on_pty_write(move |_, bytes| {
            if reply.try_send(bytes.to_vec()).is_err() {
                let _ = reply_errors.try_send(Output::Error(
                    "Terminal input queue is full or closed.".into(),
                ));
            }
        })
        .map_err(|e| e.to_string())?;
        let size = Rc::new(std::cell::Cell::new((80, 24, 0, 0)));
        let report_size = size.clone();
        vt.on_size(move |_| {
            let (columns, rows, cell_width, cell_height) = report_size.get();
            Some(libghostty_vt::terminal::SizeReportSize {
                columns,
                rows,
                cell_width,
                cell_height,
            })
        })
        .map_err(|e| e.to_string())?;
        vt.on_device_attributes(|_| {
            use libghostty_vt::terminal::*;
            Some(DeviceAttributes {
                primary: PrimaryDeviceAttributes::new(
                    ConformanceLevel::VT220,
                    &[DeviceAttributeFeature::ANSI_COLOR],
                ),
                secondary: SecondaryDeviceAttributes {
                    device_type: DeviceType::VT220,
                    firmware_version: 1,
                    rom_cartridge: 0,
                },
                tertiary: TertiaryDeviceAttributes { unit_id: 0 },
            })
        })
        .map_err(|e| e.to_string())?;
        vt.on_xtversion(|_| Some(concat!("hedit ", env!("CARGO_PKG_VERSION"))))
            .map_err(|e| e.to_string())?;
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| format!("Cannot start shell: {e}"))?;
        drop(pair.slave);
        let wake = std::sync::Arc::new(wake);
        let write_wake = wake.clone();
        let write_errors = output.clone();
        thread::spawn(move || {
            while let Ok(bytes) = writes.recv() {
                if let Err(error) = writer.write_all(&bytes).and_then(|_| writer.flush()) {
                    let _ = write_errors.send(Output::Error(format!("Terminal input: {error}")));
                    write_wake();
                    break;
                }
            }
        });
        thread::spawn(move || {
            let mut bytes = [0; IO_CHUNK];
            loop {
                match reader.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(n) => {
                        if output.send(Output::Bytes(bytes[..n].to_vec())).is_err() {
                            break;
                        }
                        wake();
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    // Unix PTYs report EIO when the slave closes.
                    Err(e) if cfg!(unix) && e.raw_os_error() == Some(5) => break,
                    Err(e) => {
                        let _ = output.send(Output::Error(format!("Terminal output: {e}")));
                        break;
                    }
                }
            }
            wake();
        });
        Ok(Self {
            vt,
            render,
            row_iter,
            cell_iter,
            encoder,
            master: pair.master,
            child: Some(child),
            input,
            output: reads,
            directory: directory.into(),
            status: "Running".into(),
            running: true,
            output_generation: 0,
            size,
        })
    }

    /// Bounded work per frame, including while the panel is hidden.
    pub fn poll(&mut self) -> bool {
        let mut count = 0;
        while count < QUEUE_SIZE {
            let Ok(output) = self.output.try_recv() else {
                break;
            };
            match output {
                Output::Bytes(bytes) => {
                    self.vt.vt_write(&bytes);
                    self.output_generation = self.output_generation.wrapping_add(1);
                }
                Output::Error(error) => self.status = error,
            }
            count += 1;
        }
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.status = format!("Shell exited: {status}");
                    self.running = false;
                    self.child = None;
                }
                Err(error) => self.status = format!("Shell status: {error}"),
                Ok(None) => {}
            }
        }
        count == QUEUE_SIZE
    }

    pub fn write(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        if !self.running {
            return Err("The shell has exited. Start a new terminal.".into());
        }
        if bytes.len() > PASTE_LIMIT * 2 {
            return Err("Terminal input is too large.".into());
        }
        self.input
            .try_send(bytes)
            .map_err(|_| "Terminal input queue is full or closed; try again.".to_owned())?;
        self.vt.scroll_viewport(ScrollViewport::Bottom);
        let _ = self.vt.set_selection(None);
        Ok(())
    }

    pub fn paste(&mut self, text: &str) -> Result<(), String> {
        if text.len() > PASTE_LIMIT {
            return Err("Paste is limited to 256 KiB.".into());
        }
        let mut text = text.as_bytes().to_vec();
        let mut encoded = vec![0; text.len() * 2 + 32];
        let bracketed = self
            .vt
            .mode(libghostty_vt::terminal::Mode::BRACKETED_PASTE)
            .map_err(|e| e.to_string())?;
        let len = paste::encode(&mut text, bracketed, &mut encoded).map_err(|e| e.to_string())?;
        encoded.truncate(len);
        self.write(encoded)
    }

    pub fn key(&mut self, event: &key::Event<'_>) -> Result<(), String> {
        let mut bytes = Vec::new();
        self.encoder
            .set_options_from_terminal(&self.vt)
            .encode_to_vec(event, &mut bytes)
            .map_err(|e| e.to_string())?;
        if !bytes.is_empty() {
            self.write(bytes)?;
        }
        Ok(())
    }

    pub fn resize(&mut self, cols: u16, rows: u16, width: u32, height: u32) -> Result<(), String> {
        let size = (cols, rows, width, height);
        if self.size.get() == size {
            return Ok(());
        }
        self.master
            .resize(PtySize {
                cols,
                rows,
                pixel_width: (u32::from(cols) * width).min(u32::from(u16::MAX)) as u16,
                pixel_height: (u32::from(rows) * height).min(u32::from(u16::MAX)) as u16,
            })
            .map_err(|e| e.to_string())?;
        self.vt
            .resize(cols, rows, width, height)
            .map_err(|e| e.to_string())?;
        self.size.set(size);
        Ok(())
    }

    pub fn grid(&mut self) -> Result<Grid, libghostty_vt::Error> {
        let snapshot = self.render.update(&self.vt)?;
        let colors = snapshot.colors()?;
        let cursor = if snapshot.cursor_visible()? {
            snapshot.cursor_viewport()?.map(|c| (c.x, c.y))
        } else {
            None
        };
        let mut rows = Vec::new();
        let mut continuations = Vec::new();
        let mut row_iter = self.row_iter.update(&snapshot)?;
        while let Some(row) = row_iter.next() {
            continuations.push(row.raw_row()?.is_wrap_continuation()?);
            let mut cells = Vec::new();
            let mut cell_iter = self.cell_iter.update(row)?;
            while let Some(cell) = cell_iter.next() {
                let wide = cell.raw_cell()?.wide()?;
                cells.push(Cell {
                    text: if matches!(wide, CellWide::SpacerTail | CellWide::SpacerHead) {
                        String::new()
                    } else {
                        cell.graphemes()?.into_iter().collect()
                    },
                    foreground: cell.fg_color()?.unwrap_or(colors.foreground),
                    background: cell.bg_color()?.unwrap_or(colors.background),
                    style: cell.style()?,
                    wide: wide == CellWide::Wide,
                    selected: cell.is_selected()?,
                });
            }
            rows.push(cells);
        }
        Ok(Grid {
            rows,
            continuations,
            background: colors.background,
            cursor,
        })
    }

    /// Terminate this shell while retaining its screen and scrollback.
    pub fn stop(&mut self) {
        self.hang_up();
        self.running = false;
        self.status = "Stopped".into();
    }

    fn hang_up(&mut self) {
        if self.child.is_none() {
            return;
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if let Some(group) = self
            .master
            .process_group_leader()
            .filter(|&pid| pid > 1)
            .and_then(rustix::process::Pid::from_raw)
        {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::HUP);
        }
        if let Some(mut child) = self.child.take() {
            thread::spawn(move || {
                let _ = child.kill();
                let _ = child.wait();
            });
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // The foreground job can have a different process group from the shell.
        // Hang up that group too, as a terminal window does when it closes.
        self.hang_up();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::time::{Duration, Instant};

    #[test]
    fn ghostty_encodes_control_keys_application_arrows_and_bracketed_paste() {
        let mut vt = Terminal::new(TerminalOptions {
            cols: 80,
            rows: 24,
            max_scrollback: 100,
        })
        .unwrap();
        let mut encoder = key::Encoder::new().unwrap();
        let mut event = key::Event::new().unwrap();
        event
            .set_action(key::Action::Press)
            .set_key(key::Key::C)
            .set_mods(key::Mods::CTRL)
            .set_utf8(Some("c"));
        let mut encoded = Vec::new();
        encoder
            .set_options_from_terminal(&vt)
            .encode_to_vec(&event, &mut encoded)
            .unwrap();
        assert_eq!(encoded, b"\x03");
        vt.vt_write(b"\x1b[?1h\x1b[?2004h");
        event
            .set_key(key::Key::ArrowUp)
            .set_mods(key::Mods::empty())
            .set_utf8(None::<String>);
        encoded.clear();
        encoder
            .set_options_from_terminal(&vt)
            .encode_to_vec(&event, &mut encoded)
            .unwrap();
        assert_eq!(encoded, b"\x1bOA");
        assert!(vt
            .mode(libghostty_vt::terminal::Mode::BRACKETED_PASTE)
            .unwrap());
        let mut text = b"one\ntwo".to_vec();
        let mut buffer = [0; 100];
        let len = paste::encode(&mut text, true, &mut buffer).unwrap();
        assert!(buffer[..len].starts_with(b"\x1b[200~"));
        assert!(buffer[..len].ends_with(b"\x1b[201~"));
    }

    #[cfg(unix)]
    fn shell(directory: &Path, script: &str) -> Session {
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", script]);
        Session::spawn(directory, command, || {}).unwrap()
    }

    #[cfg(unix)]
    fn wait_for(session: &mut Session, needle: &str) {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            session.poll();
            let grid = session.grid().unwrap();
            let text: String = grid
                .rows
                .iter()
                .flat_map(|row| row.iter().map(|c| c.text.as_str()))
                .collect();
            if text.contains(needle) {
                return;
            }
            assert!(
                Instant::now() < until,
                "waiting for {needle:?}: {} ({text})",
                session.status
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[test]
    fn pty_shell_uses_directory_accepts_input_resizes_and_reports_exit() {
        let directory = tempfile::tempdir().unwrap();
        let mut session = shell(directory.path(), "stty -echo; printf 'READY\\n'; read value; stty size; printf 'INPUT:%s\\n' \"$value\"; pwd; exit 7");
        wait_for(&mut session, "READY");
        session.resize(100, 30, 8, 16).unwrap();
        session.paste("hello\n").unwrap();
        wait_for(&mut session, "INPUT:hello");
        wait_for(&mut session, "30 100");
        wait_for(&mut session, directory.path().to_str().unwrap());
        let until = Instant::now() + Duration::from_secs(5);
        while session.running && Instant::now() < until {
            session.poll();
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!session.running);
        assert!(session.status.contains('7'), "{}", session.status);
        assert!(session.paste("more").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn terminal_query_responses_reach_the_pty() {
        let directory = tempfile::tempdir().unwrap();
        let mut session = shell(
            directory.path(),
            "stty -echo -icanon; printf READY; dd bs=1 count=4 2>/dev/null | od -An -tx1 | tr -d '[:space:]'",
        );
        wait_for(&mut session, "READY");
        session.vt.vt_write(b"\x1b[5n");
        wait_for(&mut session, "1b5b306e");
    }

    #[cfg(unix)]
    #[test]
    fn grid_preserves_ansi_colors_wide_text_and_alternate_screen() {
        let directory = tempfile::tempdir().unwrap();
        let mut session = shell(directory.path(), "stty -echo; printf READY; read value");
        wait_for(&mut session, "READY");
        session.vt.reset();
        session.vt.vt_write("\x1b[38;2;1;2;3mA中\x1b[0m".as_bytes());
        let grid = session.grid().unwrap();
        assert_eq!(grid.rows[0][0].text, "A");
        assert_eq!(grid.rows[0][0].foreground, RgbColor { r: 1, g: 2, b: 3 });
        assert_eq!(grid.rows[0][1].text, "中");
        assert!(grid.rows[0][1].wide);
        assert!(grid.rows[0][2].text.is_empty());
        session.vt.vt_write(b"\x1b[?1049h\x1b[2J\x1b[HFULLSCREEN");
        let text: String = session.grid().unwrap().rows[0]
            .iter()
            .map(|c| c.text.as_str())
            .collect();
        assert!(text.starts_with("FULLSCREEN"));
        session.vt.vt_write(b"\x1b[?1049l");
        assert_eq!(session.grid().unwrap().rows[0][0].text, "A");
        assert!(session.paste(&"x".repeat(PASTE_LIMIT + 1)).is_err());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn closing_a_terminal_terminates_and_reaps_its_process() {
        let directory = tempfile::tempdir().unwrap();
        let mut session = shell(directory.path(), "printf READY; exec sleep 60");
        wait_for(&mut session, "READY");
        let pid = rustix::process::Pid::from_raw(
            session.child.as_ref().unwrap().process_id().unwrap() as i32,
        )
        .unwrap();
        drop(session);
        let until = Instant::now() + Duration::from_secs(5);
        while rustix::process::test_kill_process(pid).is_ok() {
            assert!(
                Instant::now() < until,
                "the terminal process was not reaped"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}
