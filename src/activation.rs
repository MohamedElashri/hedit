//! Present windows for explicit launches, including requests forwarded on Wayland.
use eframe::egui;
use serde::{Deserialize, Serialize};

/// Tokens belong to one launch, not to the long-lived editor process.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Launch {
    pub wayland: Option<String>,
}

impl Launch {
    /// Called in the detached child before any GUI or worker threads. Clear the
    /// environment so restored windows, builds and terminals cannot reuse it.
    pub fn take_from_env() -> Self {
        #[cfg(target_os = "linux")]
        {
            let launch = Self {
                wayland: std::env::var("XDG_ACTIVATION_TOKEN")
                    .ok()
                    .filter(|s| !s.is_empty()),
            };
            winit::platform::startup_notify::reset_activation_token_env();
            launch
        }
        #[cfg(not(target_os = "linux"))]
        Self::default()
    }
}

#[derive(Default)]
pub struct Window {
    pending: Option<Launch>,
    presented: bool,
    #[cfg(target_os = "linux")]
    wayland: Option<wayland::Activation>,
}

impl Window {
    /// Borrow native handles for the root window.
    ///
    /// # Safety
    /// The returned value must be dropped before eframe destroys that window
    /// or its display connection.
    pub unsafe fn new(cc: &eframe::CreationContext<'_>) -> Self {
        #[cfg(not(target_os = "linux"))]
        let _ = cc;
        Self {
            pending: None,
            presented: false,
            #[cfg(target_os = "linux")]
            wayland: wayland::Activation::new(cc),
        }
    }

    pub fn request(&mut self, ctx: &egui::Context, launch: Launch) {
        self.pending = Some(launch);
        self.presented = false;
        ctx.request_repaint();
    }

    /// Present after a frame has mapped the window. Each launch requests focus
    /// once, so normal background updates never take it back from the user.
    pub fn update(&mut self, ctx: &egui::Context) {
        let Some(launch) = self.pending.as_ref() else {
            return;
        };
        if !self.presented {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            self.presented = true;
            ctx.request_repaint();
            return;
        }
        #[allow(unused_mut)]
        let mut activated = false;
        #[cfg(target_os = "linux")]
        if let (Some(wayland), Some(token)) = (&mut self.wayland, &launch.wayland) {
            match wayland.activate(token) {
                Some(false) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(16));
                    return;
                }
                Some(true) => activated = true,
                None => {}
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = launch;
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        // winit's Focus is unsupported on Wayland. Its attention request uses
        // xdg-activation when no token was supplied by the initiating terminal.
        if !activated {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
        }
        self.pending = None;
    }
}

#[cfg(target_os = "linux")]
mod wayland {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
    use wayland_client::{
        backend::{Backend, ObjectId},
        protocol::{wl_callback, wl_registry, wl_surface},
        Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    };
    use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::XdgActivationV1;

    pub(super) struct Activation {
        connection: Connection,
        queue: EventQueue<State>,
        state: State,
        surface: wl_surface::WlSurface,
        registry: wl_registry::WlRegistry,
        sync: wl_callback::WlCallback,
        started: std::time::Instant,
    }

    #[derive(Default)]
    struct State {
        manager: Option<XdgActivationV1>,
        ready: bool,
    }

    impl Activation {
        pub(super) fn new(cc: &eframe::CreationContext<'_>) -> Option<Self> {
            let RawDisplayHandle::Wayland(display) = cc.display_handle().ok()?.as_raw() else {
                return None;
            };
            let RawWindowHandle::Wayland(window) = cc.window_handle().ok()?.as_raw() else {
                return None;
            };
            // SAFETY: eframe keeps the root display and surface alive while the
            // app exists. This guest backend borrows them and never disconnects
            // the display or takes ownership of winit's surface/event handling.
            let (backend, surface) = unsafe {
                (
                    Backend::from_foreign_display(display.display.as_ptr().cast()),
                    ObjectId::from_ptr(
                        wl_surface::WlSurface::interface(),
                        window.surface.as_ptr().cast(),
                    )
                    .ok()?,
                )
            };
            let connection = Connection::from_backend(backend);
            let surface = wl_surface::WlSurface::from_id(&connection, surface).ok()?;
            let queue = connection.new_event_queue();
            let handle = queue.handle();
            let registry = connection.display().get_registry(&handle, ());
            let sync = connection.display().sync(&handle, ());
            connection.flush().ok()?;
            Some(Self {
                connection,
                queue,
                state: State::default(),
                surface,
                registry,
                sync,
                started: std::time::Instant::now(),
            })
        }

        /// Discover the activation global without a blocking round trip on the
        /// UI thread. winit reads the shared socket; we dispatch only our queue.
        pub(super) fn activate(&mut self, token: &str) -> Option<bool> {
            if self.connection.backend().dispatch_inner_queue().is_err()
                || self.queue.dispatch_pending(&mut self.state).is_err()
            {
                return None;
            }
            if !self.state.ready && self.started.elapsed() < std::time::Duration::from_secs(1) {
                return Some(false);
            }
            let manager = self.state.manager.as_ref()?;
            manager.activate(token.into(), &self.surface);
            self.connection.flush().ok()?;
            Some(true)
        }
    }

    impl Drop for Activation {
        fn drop(&mut self) {
            if let Some(manager) = &self.state.manager {
                manager.destroy();
            }
            let backend = self.connection.backend();
            let _ = backend.destroy_object(&self.registry.id());
            let _ = backend.destroy_object(&self.sync.id());
            let _ = self.connection.flush();
        }
    }

    impl Dispatch<wl_registry::WlRegistry, ()> for State {
        fn event(
            state: &mut Self,
            registry: &wl_registry::WlRegistry,
            event: wl_registry::Event,
            _: &(),
            _: &Connection,
            handle: &QueueHandle<Self>,
        ) {
            if let wl_registry::Event::Global {
                name, interface, ..
            } = event
            {
                if interface == XdgActivationV1::interface().name {
                    state.manager = Some(registry.bind(name, 1, handle, ()));
                }
            }
        }
    }

    impl Dispatch<wl_callback::WlCallback, ()> for State {
        fn event(
            state: &mut Self,
            _: &wl_callback::WlCallback,
            _: wl_callback::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            state.ready = true;
        }
    }

    wayland_client::delegate_noop!(State: ignore XdgActivationV1);

    #[cfg(test)]
    mod tests {
        use super::super::*;
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };

        #[test]
        #[ignore = "opens an isolated, automatically closing window on a Wayland desktop"]
        fn native_wayland_activation() {
            struct App {
                activation: Window,
                finished: Arc<AtomicBool>,
                focused: Arc<AtomicBool>,
                protocol_checked: bool,
                since: std::time::Instant,
            }
            impl eframe::App for App {
                fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
                    self.activation.update(ctx);
                    if !self.protocol_checked && self.activation.pending.is_none() {
                        assert!(self
                            .activation
                            .wayland
                            .as_ref()
                            .unwrap()
                            .state
                            .manager
                            .is_some());
                        self.protocol_checked = true;
                        self.activation.request(ctx, Launch::default());
                    }
                    if ctx.input(|i| i.focused) {
                        self.focused.store(true, Ordering::Relaxed);
                    }
                    if self.protocol_checked
                        && self.activation.pending.is_none()
                        && self.since.elapsed() > std::time::Duration::from_millis(750)
                    {
                        self.finished.store(true, Ordering::Relaxed);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    } else if self.since.elapsed() > std::time::Duration::from_secs(3) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    ctx.request_repaint_after(std::time::Duration::from_millis(16));
                }
                fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
                    self.activation = Window::default();
                }
            }
            let directory = tempfile::tempdir().unwrap();
            let finished = Arc::new(AtomicBool::new(false));
            let checked = finished.clone();
            let focused = Arc::new(AtomicBool::new(false));
            let observed = focused.clone();
            let options = eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_inner_size([320.0, 100.0])
                    .with_app_id("hedit-activation-test")
                    .with_active(false),
                persistence_path: Some(directory.path().join("window.ron")),
                event_loop_builder: Some(Box::new(|builder| {
                    use winit::platform::wayland::EventLoopBuilderExtWayland;
                    builder.with_wayland().with_any_thread(true);
                })),
                ..Default::default()
            };
            eframe::run_native(
                "hedit activation check",
                options,
                Box::new(move |cc| {
                    // SAFETY: App::on_exit drops this before eframe's native window.
                    let mut activation = unsafe { Window::new(cc) };
                    // Unknown tokens are ignored by the compositor. This verifies
                    // our protocol binding, borrowed handles and cleanup without
                    // relying on the desktop granting a synthetic focus request.
                    activation.request(
                        &cc.egui_ctx,
                        Launch {
                            wayland: Some("hedit-activation-smoke-test".into()),
                        },
                    );
                    Ok(Box::new(App {
                        activation,
                        finished,
                        focused,
                        protocol_checked: false,
                        since: std::time::Instant::now(),
                    }))
                }),
            )
            .unwrap();
            assert!(
                checked.load(Ordering::Relaxed),
                "native activation completed"
            );
            eprintln!(
                "Desktop granted focus during the smoke test: {}",
                observed.load(Ordering::Relaxed)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commands(ctx: &egui::Context, window: &mut Window) -> Vec<egui::ViewportCommand> {
        ctx.run(Default::default(), |ctx| window.update(ctx))
            .viewport_output
            .remove(&egui::ViewportId::ROOT)
            .unwrap()
            .commands
    }

    #[test]
    fn presentation_restores_before_focusing_and_does_not_repeat() {
        let ctx = egui::Context::default();
        let mut window = Window::default();
        for _ in 0..2 {
            window.request(&ctx, Launch::default());
            let first = commands(&ctx, &mut window);
            assert!(first
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::Visible(true))));
            assert!(first
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::Minimized(false))));
            assert!(!first
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::Focus)));
            let next = commands(&ctx, &mut window);
            assert!(next
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::Focus)));
            assert!(next
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::RequestUserAttention(_))));
            assert!(commands(&ctx, &mut window).is_empty());
        }
    }

    #[test]
    fn older_ipc_requests_need_no_activation_token() {
        let request: crate::ipc::Request =
            serde_json::from_str(r#"{"files":[],"stdin":null,"new_buffer":false}"#).unwrap();
        assert!(request.activation.wayland.is_none());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn environment_fixture() {
        if std::env::var_os("HEDIT_TEST_ACTIVATION").is_none() {
            return;
        }
        let launch = Launch::take_from_env();
        assert_eq!(launch.wayland.as_deref(), Some("one-launch-token"));
        assert!(std::env::var_os("XDG_ACTIVATION_TOKEN").is_none());
        assert!(std::env::var_os("DESKTOP_STARTUP_ID").is_none());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn launch_tokens_are_consumed_before_starting_children() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "activation::tests::environment_fixture",
                "--nocapture",
            ])
            .env("HEDIT_TEST_ACTIVATION", "1")
            .env("XDG_ACTIVATION_TOKEN", "one-launch-token")
            .env("DESKTOP_STARTUP_ID", "one-startup-id")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
