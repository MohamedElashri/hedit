#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
#[cfg(test)]
mod preview;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = hedit::cli::Options::parse(std::env::args_os().skip(1))?;
    if options.help {
        println!("hedit — keep your train of thought\n\nUsage: hedit [--session-dir DIR] [--new] [FILE[:LINE[:COLUMN]] | FOLDER ...] [-]\n\n  -                 Read UTF-8 stdin into an unsaved file\n  --new             Add a new file\n  --session-dir DIR Use an isolated session (one process per session)\n  --                Treat remaining arguments as paths\n  -h, --help        Show help\n  -V, --version     Show version\n\nBuild and run with Cargo. CUDA tools are optional.");
        return Ok(());
    }
    if options.version {
        println!("hedit {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let directory = options
        .session_dir
        .clone()
        .or_else(|| std::env::var_os("HEDIT_SESSION_DIR").map(Into::into))
        .or_else(|| {
            directories::ProjectDirs::from("", "hedit", "hedit")
                .map(|d| d.data_local_dir().join("session"))
        })
        .ok_or("Cannot determine session directory; pass --session-dir.")?;
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([720.0, 480.0])
            .with_app_id("hedit")
            .with_icon(std::sync::Arc::new(app::icon())),
        persistence_path: Some(directory.join("window.ron")),
        ..Default::default()
    };
    // Acquire the session lock before starting the UI. A second process must
    // never overwrite this process's snapshots.
    let (mut workspace, errors) = hedit::workspace::Workspace::restore(directory)?;
    if options.new_buffer {
        workspace.new_buffer()?;
    }
    if options.stdin {
        use std::io::Write;
        let mut spool = tempfile::Builder::new().prefix("hedit-stdin-").tempfile()?;
        std::io::copy(&mut std::io::stdin().lock(), &mut spool)?;
        spool.flush()?;
        let mut doc = hedit::document::Document::from_stdin(workspace.editor.id(), spool.path())?;
        let page = doc.page(0)?;
        workspace.add(doc, page)?;
    }
    // Stdin has already been consumed and its spool removed. Preserve it even
    // if creating the native window fails (for example, no graphical display).
    workspace.checkpoint()?;
    eframe::run_native(
        "hedit",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(app::HeditApp::new(
                cc,
                workspace,
                errors,
                options.files,
            )))
        }),
    )?;
    Ok(())
}
