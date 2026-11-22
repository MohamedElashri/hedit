use std::io::{Read, Write};
fn main() {
    if std::env::args().nth(1).as_deref() == Some("--stderr-flood") {
        std::io::stderr().write_all(&[b'x'; 8192]).unwrap();
        std::io::stderr().flush().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(30));
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--stream") {
        let mut input = std::io::stdin().lock();
        let mut output = std::io::stdout().lock();
        loop {
            let mut length = [0; 4];
            if input.read_exact(&mut length).is_err() { return; }
            let length = u32::from_le_bytes(length) as usize;
            assert!(length <= 65536);
            let mut bytes = vec![0; length];
            input.read_exact(&mut bytes).unwrap();
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
