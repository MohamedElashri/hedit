use hedit_remote::{
    client::{Client, SshTarget},
    protocol::{self, Error, Request},
};
use std::{
    io::{self, BufRead, Read, Write},
    path::PathBuf,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("hedit-remote: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Error> {
    let mut args = std::env::args().skip(1);
    let Some(mode) = args.next() else {
        return Err(Error::Protocol);
    };
    if mode == "--help" {
        println!("hedit SSH workspace helper\n\
            worker --state ABSOLUTE_PRIVATE_DIRECTORY [--idle-seconds N]\n\
            bridge --state ABSOLUTE_PRIVATE_DIRECTORY\n\
            connect --host HOST --helper ABSOLUTE_REMOTE_EXECUTABLE --state ABSOLUTE_REMOTE_DIRECTORY\n\
            [--port N] [--user USER] [--ssh-config ABSOLUTE_LOCAL_CONFIG]\n\
            connect accepts one bounded JSON Request per line and prints its Result.\n\
            The editor uses this helper for host-only recovery, files, search, Git, builds, and terminal streams. Install it explicitly on the host.");
        return Ok(());
    }
    let (mut state, mut host, mut helper, mut user, mut config) = (None, None, None, None, None);
    let (mut port, mut idle) = (22, 300_u64);
    while let Some(key) = args.next() {
        let value = args.next().ok_or(Error::Protocol)?;
        match key.as_str() {
            "--state" => state = Some(value),
            "--host" => host = Some(value),
            "--helper" => helper = Some(value),
            "--user" => user = Some(value),
            "--ssh-config" => config = Some(PathBuf::from(value)),
            "--port" => port = value.parse().map_err(|_| Error::Protocol)?,
            "--idle-seconds" => idle = value.parse().map_err(|_| Error::Protocol)?,
            _ => return Err(Error::Protocol),
        }
    }
    let state = state.ok_or(Error::Protocol)?;
    if !(1..=3600).contains(&idle) {
        return Err(Error::Protocol);
    }
    match mode.as_str() {
        #[cfg(unix)]
        "worker" => hedit_remote::server::run(
            std::path::Path::new(&state),
            std::time::Duration::from_secs(idle),
        ),
        #[cfg(unix)]
        "bridge" => hedit_remote::server::bridge(std::path::Path::new(&state)),
        "connect" => {
            let target = SshTarget {
                host: host.ok_or(Error::Protocol)?,
                helper: helper.ok_or(Error::Protocol)?,
                state,
                user,
                port,
                config,
            };
            let mut client = Client::connect(&target)?;
            let mut input = io::BufReader::new(io::stdin().lock());
            let mut output = io::stdout().lock();
            loop {
                let mut line = Vec::new();
                let count = Read::by_ref(&mut input)
                    .take(protocol::MAX_FRAME as u64 + 1)
                    .read_until(b'\n', &mut line)
                    .map_err(|_| Error::Protocol)?;
                if count == 0 {
                    break;
                }
                if count > protocol::MAX_FRAME || !line.ends_with(b"\n") {
                    return Err(Error::Limit);
                }
                let request: Request =
                    serde_json::from_slice(&line).map_err(|_| Error::Protocol)?;
                let result = client.call(request);
                serde_json::to_writer(&mut output, &result).map_err(|_| Error::Protocol)?;
                output
                    .write_all(b"\n")
                    .and_then(|_| output.flush())
                    .map_err(|_| Error::Unavailable)?;
            }
            Ok(())
        }
        _ => Err(Error::Unsupported),
    }
}
