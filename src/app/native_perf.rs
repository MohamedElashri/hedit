//! Opt-in, local numeric diagnostics. No source, paths or input values are sampled.
use eframe::{egui, egui_glow, glow};
use glow::HasContext as _;
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    io::{self, BufWriter, Write},
    path::Path,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Instant,
};

const QUEUE: usize = 128;
const GPU_PENDING: usize = 32;
const MAX_TRACE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default, Clone, Copy, Serialize)]
struct Counts {
    keys: u32,
    text_events: u32,
    pastes: u32,
    scrolls: u32,
    pointer_events: u32,
}
impl Counts {
    fn from_input(input: &egui::RawInput) -> Self {
        let mut counts = Self::default();
        for event in &input.events {
            match event {
                egui::Event::Key { pressed: true, .. } => counts.keys += 1,
                egui::Event::Text(_) | egui::Event::Ime(egui::ImeEvent::Commit(_)) => {
                    counts.text_events += 1
                }
                egui::Event::Paste(_) => counts.pastes += 1,
                egui::Event::MouseWheel { .. } => counts.scrolls += 1,
                egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. } => {
                    counts.pointer_events += 1
                }
                _ => (),
            }
        }
        counts
    }
}
#[derive(Serialize)]
struct Sample {
    sequence: u64,
    #[serde(flatten)]
    counts: Counts,
    cpu_to_paint_us: u64,
    gpu_draw_ns: Option<u64>,
    gpu_timer_supported: bool,
    dropped_samples: u64,
}
struct Input {
    sequence: u64,
    counts: Counts,
    start: Instant,
}
struct Pending {
    query: glow::NativeQuery,
    sample: Sample,
}
struct State {
    sender: Option<mpsc::SyncSender<Sample>>,
    input: Option<Input>,
    sequence: u64,
    active: Option<glow::NativeQuery>,
    pending: Vec<Pending>,
    gpu_supported: bool,
    dropped: u64,
}
impl State {
    fn emit(&mut self, mut sample: Sample) {
        sample.dropped_samples = self.dropped;
        match self.sender.as_ref().map(|tx| tx.try_send(sample)) {
            Some(Err(mpsc::TrySendError::Full(_))) => self.dropped += 1,
            Some(Err(mpsc::TrySendError::Disconnected(_))) => {
                self.dropped += 1;
                self.sender = None;
            }
            _ => (),
        }
    }
    fn poll(&mut self, gl: &glow::Context, closing: bool) {
        let pending = std::mem::take(&mut self.pending);
        for mut item in pending {
            // SAFETY: only called from paint callbacks or on_exit with the
            // app's current GL context. Result reads never wait for the GPU.
            // glow 0.16's u32 wrapper tests the wrong core function before
            // selecting its EXT fallback on core 4.1 contexts. Its u64 wrapper
            // checks the actual entry point, including for availability reads.
            let available = query_parameter(gl, item.query, glow::QUERY_RESULT_AVAILABLE);
            let ready = available.is_some_and(|v| v != 0);
            if ready || closing || available.is_none() {
                if ready {
                    item.sample.gpu_draw_ns = query_parameter(gl, item.query, glow::QUERY_RESULT);
                }
                unsafe {
                    gl.delete_query(item.query);
                }
                self.emit(item.sample);
            } else {
                self.pending.push(item);
            }
        }
    }
    fn begin_paint(&mut self, gl: &glow::Context) {
        // Recover if a previous frame did not reach its final paint callback.
        if let Some(query) = self.active.take() {
            unsafe {
                gl.end_query(glow::TIME_ELAPSED);
                gl.delete_query(query);
            }
        }
        if self.sender.is_none() {
            self.poll(gl, true);
            return;
        }
        self.poll(gl, false);
        if self.sender.is_none() {
            self.poll(gl, true);
            return;
        }
        if self.gpu_supported && self.pending.len() < GPU_PENDING {
            match unsafe { gl.create_query() } {
                Ok(query) => {
                    unsafe {
                        gl.begin_query(glow::TIME_ELAPSED, query);
                    }
                    self.active = Some(query);
                }
                Err(_) => self.gpu_supported = false,
            }
        }
    }
    fn end_paint(&mut self, gl: &glow::Context) {
        let query = self.active.take();
        if query.is_some() {
            unsafe {
                gl.end_query(glow::TIME_ELAPSED);
            }
        }
        let Some(input) = self.input.take() else {
            if let Some(query) = query {
                unsafe {
                    gl.delete_query(query);
                }
            }
            return;
        };
        let sample = Sample {
            sequence: input.sequence,
            counts: input.counts,
            cpu_to_paint_us: input.start.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
            gpu_draw_ns: None,
            gpu_timer_supported: self.gpu_supported,
            dropped_samples: 0,
        };
        if let Some(query) = query {
            self.pending.push(Pending { query, sample });
        } else {
            self.emit(sample);
        }
    }
}

fn query_parameter(gl: &glow::Context, query: glow::NativeQuery, parameter: u32) -> Option<u64> {
    let version = gl.version();
    let query_buffers = version.major > 4
        || (version.major == 4 && version.minor >= 4)
        || gl
            .supported_extensions()
            .contains("GL_ARB_query_buffer_object");
    // A bound query buffer would interpret the output address as a GPU-buffer
    // offset. Leave that state untouched and omit this optional sample.
    if query_buffers && unsafe { gl.get_parameter_i32(glow::QUERY_BUFFER_BINDING) } != 0 {
        return None;
    }
    let mut ns = 0_u64;
    // SAFETY: glow 0.16 exposes glGetQueryObjectui64v through its offset form.
    // With no query buffer bound, the argument is a writable CPU address.
    // `ns` is aligned, lives through this synchronous call, and is one u64.
    // QUERY_RESULT_AVAILABLE is nonblocking. The caller checks it on this
    // same current GL context before requesting QUERY_RESULT.
    unsafe {
        gl.get_query_parameter_u64_with_offset(
            query,
            parameter,
            std::ptr::from_mut(&mut ns) as usize,
        );
    }
    Some(ns)
}

pub(super) struct Trace {
    state: Arc<Mutex<State>>,
    writer: Option<thread::JoinHandle<()>>,
}
impl Trace {
    pub(super) fn from_env(cc: &eframe::CreationContext<'_>) -> Option<Self> {
        let path = std::env::var_os("HEDIT_NATIVE_TRACE_FILE")?;
        let gl = cc.gl.as_deref();
        if gl.is_none() {
            eprintln!("hedit: native numeric trace requires the OpenGL renderer");
            return None;
        }
        match Self::open(Path::new(&path), gl) {
            Ok(trace) => {
                // Version support is not enough if a loader omitted a symbol.
                let loaded = cc.get_proc_address.as_ref().is_some_and(|get| {
                    [
                        c"glGenQueries",
                        c"glDeleteQueries",
                        c"glBeginQuery",
                        c"glEndQuery",
                        c"glGetQueryObjectui64v",
                    ]
                    .iter()
                    .all(|name| !get(name).is_null())
                });
                trace.state.lock().unwrap().gpu_supported &= loaded;
                Some(trace)
            }
            Err(error) => {
                eprintln!("hedit: native numeric trace disabled: {error}");
                None
            }
        }
    }
    fn open(path: &Path, gl: Option<&glow::Context>) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        let (sender, receiver) = mpsc::sync_channel(QUEUE);
        let writer = thread::Builder::new()
            .name("native-numeric-trace".into())
            .spawn(move || write_samples(file, receiver, MAX_TRACE_BYTES))?;
        // Use core timer queries only, avoiding extension-function assumptions.
        let gpu_supported = gl.is_some_and(|gl| {
            !gl.version().is_embedded
                && (gl.version().major > 3 || (gl.version().major == 3 && gl.version().minor >= 3))
        });
        Ok(Self {
            state: Arc::new(Mutex::new(State {
                sender: Some(sender),
                input: None,
                sequence: 0,
                active: None,
                pending: Vec::with_capacity(GPU_PENDING),
                gpu_supported,
                dropped: 0,
            })),
            writer: Some(writer),
        })
    }
    pub(super) fn input(&self, raw: &egui::RawInput) {
        let start = Instant::now();
        let counts = Counts::from_input(raw);
        let mut state = self.state.lock().unwrap();
        state.sequence += 1;
        state.input = Some(Input {
            sequence: state.sequence,
            counts,
            start,
        });
    }
    pub(super) fn begin_frame(&self, ctx: &egui::Context) {
        let state = self.state.clone();
        ctx.layer_painter(egui::LayerId::background())
            .add(egui::PaintCallback {
                rect: ctx.viewport_rect(),
                callback: Arc::new(egui_glow::CallbackFn::new(move |_, painter| {
                    state.lock().unwrap().begin_paint(painter.gl());
                })),
            });
    }
    pub(super) fn end_frame(&self, ctx: &egui::Context) {
        let state = self.state.clone();
        ctx.debug_painter().add(egui::PaintCallback {
            rect: ctx.viewport_rect(),
            callback: Arc::new(egui_glow::CallbackFn::new(move |_, painter| {
                state.lock().unwrap().end_paint(painter.gl());
            })),
        });
    }
    pub(super) fn close(&mut self, gl: Option<&glow::Context>) {
        let mut state = self.state.lock().unwrap();
        if let Some(gl) = gl {
            if let Some(query) = state.active.take() {
                unsafe {
                    gl.end_query(glow::TIME_ELAPSED);
                    gl.delete_query(query);
                }
            }
            state.poll(gl, true);
        }
        state.sender.take();
        drop(state);
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}
impl Drop for Trace {
    fn drop(&mut self) {
        // Context loss or abnormal app teardown: never call GL from Drop.
        self.close(None);
    }
}
fn write_samples(file: File, receiver: mpsc::Receiver<Sample>, limit: usize) {
    let mut out = BufWriter::new(file);
    let mut bytes = 0;
    for sample in receiver {
        let Ok(line) = serde_json::to_vec(&sample) else {
            break;
        };
        if bytes + line.len() + 1 > limit {
            break;
        }
        if out.write_all(&line).is_err() || out.write_all(b"\n").is_err() || out.flush().is_err() {
            break;
        }
        bytes += line.len() + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn core_query_context(buffer_bound: bool) -> glow::Context {
        unsafe extern "system" fn version41(_: u32) -> *const u8 {
            c"4.1 test".as_ptr().cast()
        }
        unsafe extern "system" fn version46(_: u32) -> *const u8 {
            c"4.6 test".as_ptr().cast()
        }
        unsafe extern "system" fn integers(_: u32, out: *mut i32) {
            unsafe {
                out.write(0);
            }
        }
        unsafe extern "system" fn bound_integer(parameter: u32, out: *mut i32) {
            unsafe {
                out.write(if parameter == glow::QUERY_BUFFER_BINDING {
                    42
                } else {
                    0
                });
            }
        }
        unsafe extern "system" fn query(_: u32, parameter: u32, out: *mut u64) {
            unsafe {
                out.write(if parameter == glow::QUERY_RESULT_AVAILABLE {
                    1
                } else {
                    4_500_000_123
                });
            }
        }
        unsafe extern "system" fn delete(_: i32, _: *const u32) {}
        unsafe extern "system" fn error() -> u32 {
            glow::NO_ERROR
        }
        // SAFETY: these stable C-ABI stubs implement every GL function called
        // during construction or the query tests. No driver/context is used.
        unsafe {
            glow::Context::from_loader_function(|name| match name {
                "glGetString" => if buffer_bound {
                    version46 as *const ()
                } else {
                    version41 as *const ()
                }
                .cast(),
                "glGetIntegerv" => if buffer_bound {
                    bound_integer as *const ()
                } else {
                    integers as *const ()
                }
                .cast(),
                "glGetQueryObjectui64v" => (query as *const ()).cast(),
                "glDeleteQueries" => (delete as *const ()).cast(),
                "glGetError" => (error as *const ()).cast(),
                _ => std::ptr::null(), // deliberately no EXT query entry points
            })
        }
    }
    #[test]
    fn core_only_queries_poll_without_ext_symbols_and_preserve_64_bit_results() {
        let gl = core_query_context(false);
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut state = State {
            sender: Some(sender),
            input: None,
            sequence: 0,
            active: None,
            pending: vec![Pending {
                query: glow::NativeQuery(std::num::NonZeroU32::new(1).unwrap()),
                sample: Sample {
                    sequence: 1,
                    counts: Counts::default(),
                    cpu_to_paint_us: 10,
                    gpu_draw_ns: None,
                    gpu_timer_supported: true,
                    dropped_samples: 0,
                },
            }],
            gpu_supported: true,
            dropped: 0,
        };
        state.poll(&gl, false);
        assert!(state.pending.is_empty());
        assert_eq!(
            receiver.try_recv().unwrap().gpu_draw_ns,
            Some(4_500_000_123)
        );
    }
    #[test]
    fn bound_query_buffers_never_receive_a_cpu_output_address() {
        let gl = core_query_context(true);
        let query = glow::NativeQuery(std::num::NonZeroU32::new(1).unwrap());
        assert_eq!(
            query_parameter(&gl, query, glow::QUERY_RESULT_AVAILABLE),
            None
        );
        assert_eq!(query_parameter(&gl, query, glow::QUERY_RESULT), None);
    }
    #[test]
    fn trace_disk_limit_keeps_only_complete_numeric_records() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("trace");
        let file = File::create(&path).unwrap();
        let (tx, rx) = mpsc::sync_channel(100);
        for sequence in 0..100 {
            tx.send(Sample {
                sequence,
                counts: Counts::default(),
                cpu_to_paint_us: 1,
                gpu_draw_ns: Some(u64::MAX),
                gpu_timer_supported: true,
                dropped_samples: 0,
            })
            .unwrap();
        }
        drop(tx);
        write_samples(file, rx, 512);
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.is_empty() && bytes.len() <= 512);
        assert_eq!(bytes.last(), Some(&b'\n'));
        for line in std::str::from_utf8(&bytes).unwrap().lines() {
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        }
    }
    #[test]
    fn trace_records_numeric_counts_without_input_contents_and_refuses_existing_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("trace.jsonl");
        let mut trace = Trace::open(&path, None).unwrap();
        assert!(Trace::open(&path, None).is_err());
        let secret = "private draft /ssh/host/secret";
        trace.input(&egui::RawInput {
            events: vec![
                egui::Event::Text(secret.into()),
                egui::Event::Paste(secret.into()),
            ],
            ..Default::default()
        });
        let mut state = trace.state.lock().unwrap();
        let input = state.input.take().unwrap();
        state.emit(Sample {
            sequence: input.sequence,
            counts: input.counts,
            cpu_to_paint_us: 17,
            gpu_draw_ns: None,
            gpu_timer_supported: false,
            dropped_samples: 0,
        });
        drop(state);
        trace.close(None);
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(!contents.contains(secret));
        let row: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
        assert_eq!(row["text_events"], 1);
        assert_eq!(row["pastes"], 1);
        assert!(row
            .as_object()
            .unwrap()
            .values()
            .all(|v| v.is_number() || v.is_boolean() || v.is_null()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    #[cfg(unix)]
    #[test]
    fn trace_does_not_follow_a_preexisting_symlink() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target");
        std::fs::write(&target, b"keep").unwrap();
        let path = directory.path().join("trace");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(Trace::open(&path, None).is_err());
        assert_eq!(std::fs::read(target).unwrap(), b"keep");
    }
    #[test]
    fn a_backpressured_trace_drops_samples_without_blocking_the_ui() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        let mut state = State {
            sender: Some(sender),
            input: None,
            sequence: 0,
            active: None,
            pending: Vec::new(),
            gpu_supported: false,
            dropped: 0,
        };
        for n in 0..10000 {
            state.emit(Sample {
                sequence: n,
                counts: Counts::default(),
                cpu_to_paint_us: 1,
                gpu_draw_ns: None,
                gpu_timer_supported: false,
                dropped_samples: 0,
            });
        }
        assert_eq!(state.dropped, 9999);
    }
}
