#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod launcher;
#[cfg(target_os = "macos")]
mod macos_privacy;
mod plugin_runtime;
#[cfg(test)]
mod preview;
mod ssh_askpass;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    if let Some(completed) = plugin_runtime::run_if_requested(&args) {
        std::process::exit(i32::from(!completed));
    }
    if let Some(answered) = ssh_askpass::run_if_requested(&args) {
        std::process::exit(i32::from(!answered));
    }
    if args.first().is_some_and(|arg| arg == "plugin") {
        return app::run_plugin_cli(&args[1..]).map_err(Into::into);
    }
    let mut startup = launcher::Startup::take_from_args(&mut args)?;
    let result = run(args, &mut startup);
    if let Err(error) = &result {
        startup.failed(error.to_string());
    }
    result
}

fn run(
    args: Vec<std::ffi::OsString>,
    startup: &mut launcher::Startup,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = hedit::cli::Options::parse(args.clone())?;
    if options.help {
        println!("hedit — keep your train of thought\n\nUsage: hedit [--session-dir DIR] [--new] [FILE[:LINE[:COLUMN]] | FOLDER ...] [-]\n\n  -                 Read UTF-8 stdin into an unsaved file\n  --new             Add a new file\n  -w, --wait        Wait until the files are closed (for git and other tools)\n  --session-dir DIR Use an isolated session (one process per session)\n  plugin --help     Check, test, and package extensions\n  --lsp list        List registered language servers (JSON)\n  --lsp install ID  Install or update a managed language server\n  --lsp remove ID   Remove a managed server or external registration\n  --lsp-dir DIR     Override the local server store for --lsp\n  --ssh HOST        Connect; install the helper for the remote user\n  --ssh-root DIR    Remote workspace (default: remote home)\n  --ssh-helper FILE Advanced: manually installed remote helper\n  --ssh-state DIR   Advanced: private host recovery directory\n  --ssh-user USER   SSH user (optional)\n  --ssh-port PORT   SSH port (default from SSH config)\n  --ssh-config FILE Trusted absolute local SSH config\n  --ssh-use-config-jump    Use ProxyJump from local SSH config\n  --ssh-forward-config-ports  Enable loopback LocalForward rules\n  --ssh-interactive Prompt locally for SSH credentials (Linux/macOS)\n  --ssh-takeover    Explicitly fence another workspace writer\n  --                Treat remaining arguments as paths\n  -h, --help        Show help\n  -V, --version     Show version\n\nhedit opens in the running session and releases the terminal after startup.\nBuild and run with Cargo. CUDA tools are optional.");
        return Ok(());
    }
    if options.version {
        println!("hedit {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if let Some(action) = &options.language_server {
        use hedit::language_server::manager::{catalog, Manager};
        let directory = options
            .language_server_directory
            .clone()
            .or_else(|| {
                directories::ProjectDirs::from("", "hedit", "hedit")
                    .map(|d| d.data_local_dir().join("language-servers"))
            })
            .ok_or("Cannot determine language-server data directory; pass --lsp-dir.")?;
        let manager = Manager::new(directory);
        match action {
            hedit::cli::LanguageServerCommand::List => println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"catalog":catalog(),"installed":manager.list()?,"directory":manager.root()})
                )?
            ),
            hedit::cli::LanguageServerCommand::Install(id) => {
                manager.install(id, &std::sync::atomic::AtomicBool::new(false), |message| {
                    eprintln!("{message}")
                })?;
                println!("Installed {id}.");
            }
            hedit::cli::LanguageServerCommand::Remove(id) => {
                manager.remove(id)?;
                println!("Removed {id}.");
            }
        }
        return Ok(());
    }
    let remote_window = options.remote.is_some();
    let directory = options
        .session_dir
        .clone()
        .or_else(|| {
            if remote_window {
                let mut id = [0u8; 16];
                getrandom::fill(&mut id).ok()?;
                Some(std::env::temp_dir().join(format!(
                    "hedit-ssh-{}",
                    id.iter().map(|b| format!("{b:02x}")).collect::<String>()
                )))
            } else {
                std::env::var_os("HEDIT_SESSION_DIR").map(Into::into)
            }
        })
        .or_else(|| {
            directories::ProjectDirs::from("", "hedit", "hedit")
                .map(|d| d.data_local_dir().join("session"))
        })
        .ok_or("Cannot determine session directory; pass --session-dir.")?;
    if !startup.is_child() {
        let mut args = args.clone();
        if options.session_dir.is_none() {
            args.push("--session-dir".into());
            args.push(directory.clone().into_os_string());
        }
        return launcher::launch(&args, &directory, options.stdin, options.wait);
    }
    #[cfg(target_os = "macos")]
    if remote_window {
        macos_privacy::disable_cocoa_restoration();
    }
    let wait = std::env::var_os(launcher::WAIT_VARIABLE).map(std::path::PathBuf::from);
    let activation = hedit::activation::Launch::take_from_env();
    if !remote_window
        && hedit::ipc::forward(
            &directory,
            &options,
            startup.stdin_path(),
            wait.as_deref(),
            &activation,
        )?
    {
        startup.ready()?;
        return Ok(());
    }
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([720.0, 480.0])
            .with_app_id("hedit")
            .with_icon(std::sync::Arc::new(app::icon())),
        persist_window: !remote_window,
        persistence_path: Some(directory.join("window.ron")),
        ..Default::default()
    };
    // Acquire the session lock before starting the UI. A second process must
    // never overwrite this process's snapshots.
    let (mut workspace, errors) = match if let Some(config) = options.remote.clone() {
        hedit::workspace::Workspace::connect_remote(directory.clone(), config)
    } else {
        hedit::workspace::Workspace::restore(directory.clone())
    } {
        Ok(workspace) => workspace,
        Err(error) => {
            if !remote_window
                && hedit::ipc::forward(
                    &directory,
                    &options,
                    startup.stdin_path(),
                    wait.as_deref(),
                    &activation,
                )?
            {
                startup.ready()?;
                return Ok(());
            }
            return Err(error.into());
        }
    };
    // Reopen the extra windows that were open with the main window last time.
    if let Some(exe) = (!remote_window)
        .then(hedit::windows::executable)
        .and_then(Result::ok)
    {
        for session in hedit::windows::to_restore(&directory) {
            let _ = hedit::windows::spawn(&exe, &session, false);
        }
    }
    if options.new_buffer {
        workspace.new_buffer()?;
    }
    if options.stdin {
        let spool = startup.stdin_path().ok_or("Missing stdin handoff file.")?;
        let mut doc = hedit::document::Document::from_stdin(workspace.editor.id(), spool)?;
        let page = doc.page(0)?;
        workspace.add(doc, page)?;
    }
    // Stdin has already been consumed and its spool removed. Preserve it even
    // if creating the native window fails (for example, no graphical display).
    workspace.checkpoint()?;
    let paths = if let Some(remote) = &workspace.remote {
        options
            .files
            .iter()
            .map(|p| {
                p.to_str()
                    .ok_or("SSH input paths must be UTF-8.")
                    .and_then(|p| {
                        remote
                            .path(p)
                            .map_err(|_| "Invalid SSH workspace input path.")
                    })
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        options.files.clone()
    };
    let executable = hedit::windows::executable()?;
    let restart_directory = if directory.is_absolute() {
        directory.clone()
    } else {
        std::env::current_dir()?.join(&directory)
    };
    let restart_requested = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let restart = std::sync::Arc::clone(&restart_requested);
    eframe::run_native(
        "hedit",
        native_options,
        Box::new(move |cc| {
            let mut app = app::HeditApp::new(cc, workspace, errors, paths, restart_requested);
            app.request_activation(&cc.egui_ctx, activation);
            if let Some(file) = wait {
                app.wait_for(&options.files, file);
            }
            startup.ready()?;
            Ok(Box::new(app))
        }),
    )?;
    if restart.load(std::sync::atomic::Ordering::Relaxed) {
        if remote_window {
            let mut command = std::process::Command::new(&executable);
            command.args(&args);
            if options.session_dir.is_none() {
                command.arg("--session-dir").arg(&restart_directory);
            }
            let mut child = command.spawn()?;
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        } else {
            launcher::restart(&executable, &restart_directory)?;
        }
    }
    Ok(())
}
