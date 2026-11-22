//! Native reference image. The broker supplies all confinement and authority.
use std::io::{self, Read, Write};
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let result = if args == ["--version"] {
        println!("hedit-json-tools {}", env!("CARGO_PKG_VERSION"));
        Ok(())
    } else if args == ["--format"] {
        let mut bytes = Vec::new();
        io::stdin()
            .take(65537)
            .read_to_end(&mut bytes)
            .and_then(|_| hedit_json_tools::format(&bytes))
            .and_then(|text| io::stdout().write_all(text.as_bytes()))
    } else if args == ["--lsp"] {
        hedit_json_tools::serve(io::stdin().lock(), io::stdout().lock())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Usage: hedit-json-tools --format | --lsp | --version",
        ))
    };
    if result.is_err() {
        // Document text, server messages and OS paths never become diagnostics.
        eprintln!("JSON tool rejected its input or exceeded its limits.");
        std::process::exit(1);
    }
}
