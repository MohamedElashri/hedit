fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args == ["--version"] {
        println!("hedit-plugin-worker {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let completed = hedit_plugin::run_if_requested(&args).unwrap_or(false);
    std::process::exit(i32::from(!completed));
}
