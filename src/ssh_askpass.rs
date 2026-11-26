//! Short-lived local credential dialog invoked through OpenSSH's askpass pipe.
use eframe::egui;
use hedit::remote::askpass::{HOST, PROMPT_TIMEOUT, REQUEST};
use std::{
    cell::RefCell,
    ffi::{OsStr, OsString},
    io::{self, Write},
    rc::Rc,
    time::{Duration, Instant},
};

// OpenSSH's askpass reply buffer is 1024 bytes, including newline and NUL.
const MAX_REPLY: usize = 1022;

pub(super) fn run_if_requested(args: &[OsString]) -> Option<bool> {
    let requested = std::env::var_os(REQUEST)?;
    let require = std::env::var_os("SSH_ASKPASS_REQUIRE");
    let hint = std::env::var_os("SSH_ASKPASS_PROMPT");
    let host = std::env::var_os(HOST);
    Some(
        matches!(requested.as_os_str().to_str(), Some("1"))
            && require.as_deref() == Some(OsStr::new("force"))
            && handle(args, hint.as_deref(), host.as_deref()),
    )
}

fn handle(args: &[OsString], hint: Option<&OsStr>, host: Option<&OsStr>) -> bool {
    // A verified host can ask for credentials, but never use this prompt to
    // approve a key or confirm a separate SSH operation.
    let [prompt] = args else { return false };
    if hint == Some(OsStr::new("none")) {
        return true;
    }
    if hint.is_some() || !has_reply_pipe() {
        return false;
    }
    let Some(host) = host else { return false };
    show(sanitize(host, 253), sanitize(prompt, 1024))
}

fn has_reply_pipe() -> bool {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use rustix::fs::{fstat, FileType};
        fstat(io::stdout())
            .is_ok_and(|stat| FileType::from_raw_mode(stat.st_mode) == FileType::Fifo)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        false
    }
}

fn sanitize(text: &OsStr, limit: usize) -> String {
    text.to_string_lossy()
        .chars()
        .filter(|c| {
            (!c.is_control() || *c == '\n')
                && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(limit)
        .collect()
}

fn valid_reply(secret: &str) -> bool {
    secret.len() <= MAX_REPLY && !secret.contains(['\0', '\r', '\n'])
}

fn write_reply(mut writer: impl Write, answer: &str) -> io::Result<()> {
    if !valid_reply(answer) {
        return Err(io::Error::other("Invalid OpenSSH credential response."));
    }
    writer.write_all(answer.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn show(host: String, message: String) -> bool {
    #[cfg(target_os = "macos")]
    crate::macos_privacy::disable_cocoa_restoration();
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use rustix::process::{setrlimit, Resource, Rlimit};
        let _ = setrlimit(
            Resource::Core,
            Rlimit {
                current: Some(0),
                maximum: Some(0),
            },
        );
    }

    let answer = Rc::new(RefCell::new(None));
    let dialog = Prompt {
        host,
        message,
        secret: String::with_capacity(MAX_REPLY),
        answer: Rc::clone(&answer),
        deadline: Instant::now() + PROMPT_TIMEOUT,
        first_frame: true,
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        parent: rustix::process::getppid(),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("hedit-ssh-authentication")
            .with_inner_size([440.0, 240.0])
            .with_resizable(false)
            .with_always_on_top(),
        persist_window: false,
        ..Default::default()
    };
    let shown = eframe::run_native(
        "SSH authentication",
        options,
        Box::new(|_| Ok(Box::new(dialog))),
    )
    .is_ok();
    let secret = answer.borrow_mut().take();
    secret.is_some_and(|secret| shown && write_reply(io::stdout().lock(), &secret).is_ok())
}

struct Prompt {
    host: String,
    message: String,
    secret: String,
    answer: Rc<RefCell<Option<String>>>,
    deadline: Instant,
    first_frame: bool,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    parent: Option<rustix::process::Pid>,
}

impl Prompt {
    fn expired(&self) -> bool {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if self.parent != rustix::process::getppid() {
            return true;
        }
        Instant::now() >= self.deadline
    }
}

impl eframe::App for Prompt {
    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.expired() || ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        // Stop showing the dialog if its SSH parent was cancelled or timed out.
        ctx.request_repaint_after(Duration::from_millis(250));
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("SSH authentication");
            ui.label(&self.host);
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .max_height(64.0)
                .show(ui, |ui| {
                    ui.label(if self.message.trim().is_empty() {
                        "OpenSSH requested a credential."
                    } else {
                        self.message.trim()
                    });
                });
            let input = ui.add(
                egui::TextEdit::singleline(&mut self.secret)
                    .password(true)
                    .char_limit(MAX_REPLY)
                    .desired_width(f32::INFINITY),
            );
            if self.first_frame {
                input.request_focus();
                self.first_frame = false;
            }
            let valid = valid_reply(&self.secret);
            let enter = input.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                let submit = ui
                    .add_enabled(valid, egui::Button::new("Continue"))
                    .clicked();
                if submit || (valid && enter) {
                    *self.answer.borrow_mut() = Some(std::mem::take(&mut self.secret));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                } else if ui.button("Cancel").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn askpass_refuses_confirmation_and_bounds_one_line_of_output() {
        assert!(!handle(
            &["Confirm key?".into()],
            Some(OsStr::new("confirm")),
            None
        ));
        assert_eq!(sanitize(OsStr::new("\x1b[1mpass\u{202e}"), 8), "[1mpass");
        let mut bytes = Vec::new();
        for bad in [
            "one\ntwo".into(),
            "one\rtwo".into(),
            "\0".into(),
            "λ".repeat(512),
        ] {
            assert!(write_reply(&mut bytes, &bad).is_err());
            assert!(bytes.is_empty());
        }
        write_reply(&mut bytes, "fake answer λ").unwrap();
        assert_eq!(bytes, "fake answer λ\n".as_bytes());
        bytes.clear();
        write_reply(&mut bytes, "").unwrap();
        assert_eq!(bytes, b"\n");
    }
}
