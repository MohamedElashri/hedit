use std::io::{Read, Write};
fn main() {
    if std::env::args().nth(1).as_deref() == Some("--stderr-flood") {
        std::io::stderr().write_all(&[b'x'; 8192]).unwrap();
        std::io::stderr().flush().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(30));
        return;
    }
    let mode = std::env::args().nth(1);
    if matches!(mode.as_deref(), Some("--stream" | "--stream-cpu")) {
        let mut input = std::io::stdin().lock();
        let mut output = std::io::stdout().lock();
        loop {
            let mut length = [0; 4];
            if input.read_exact(&mut length).is_err() { return; }
            let length = u32::from_le_bytes(length) as usize;
            assert!(length <= 65536);
            let mut bytes = vec![0; length];
            input.read_exact(&mut bytes).unwrap();
            if mode.as_deref() == Some("--stream-cpu") {
                let before = cpu_nanos();
                while cpu_nanos() - before < 750_000_000 {
                    for _ in 0..1024 { std::hint::spin_loop(); }
                }
                std::io::stderr().write_all(&[b'x'; 2048]).unwrap();
            }
            let text = String::from_utf8(bytes).unwrap().to_uppercase();
            output.write_all(&(text.len() as u32).to_le_bytes()).unwrap();
            output.write_all(text.as_bytes()).unwrap();
            output.flush().unwrap();
        }
    }
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    std::io::stdout().write_all(input.to_uppercase().as_bytes()).unwrap();
}

#[cfg(unix)]
fn cpu_nanos() -> u64 {
    #[repr(C)] struct Timespec { seconds: i64, nanos: i64 }
    unsafe extern "C" { fn clock_gettime(clock: i32, value: *mut Timespec) -> i32; }
    let mut value = Timespec { seconds: 0, nanos: 0 };
    let clock = if cfg!(target_os = "macos") { 12 } else { 2 };
    assert_eq!(unsafe { clock_gettime(clock, &mut value) }, 0);
    value.seconds as u64 * 1_000_000_000 + value.nanos as u64
}
#[cfg(windows)]
fn cpu_nanos() -> u64 {
    #[repr(C)] #[derive(Default)] struct Filetime { low: u32, high: u32 }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn GetProcessTimes(process: *mut std::ffi::c_void, created: *mut Filetime,
            exited: *mut Filetime, kernel: *mut Filetime, user: *mut Filetime) -> i32;
    }
    let (mut created, mut exited, mut kernel, mut user) =
        (Filetime::default(), Filetime::default(), Filetime::default(), Filetime::default());
    assert_ne!(unsafe { GetProcessTimes(GetCurrentProcess(), &mut created, &mut exited, &mut kernel, &mut user) }, 0);
    let ticks = |time: Filetime| (u64::from(time.high) << 32) | u64::from(time.low);
    (ticks(kernel) + ticks(user)) * 100
}
