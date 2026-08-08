//! Wayland client: two wlr-layer-shell surfaces pinned to the left and right
//! edges of the ultrawide output, on the overlay layer so they sit above
//! fullscreen windows, with an empty input region so clicks fall through.

use anyhow::{anyhow, Context, Result};
use std::time::{Duration, Instant};

use calloop::timer::{TimeoutAction, Timer};
use calloop::{EventLoop, LoopHandle, RegistrationToken};
use calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_dispatch2, delegate_registry,
    output::{OutputHandler, OutputInfo, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_surface},
    Connection, Proxy, QueueHandle,
};

use crate::config::Config;
use crate::ipc::{CmdSender, Command};
use crate::render::{BarPlacement, EglTarget, Renderer};
use crate::tray::Tray;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Left,
    Right,
}

struct Bar {
    side: Side,
    layer: LayerSurface,
    target: Option<EglTarget>,
    /// Size from the last configure, in surface-local (logical) pixels.
    logical: (u32, u32),
    scale: i32,
}

/// The output we render onto.
struct Target {
    output: wl_output::WlOutput,
    name: String,
    /// Logical size in compositor coordinates.
    size: (u32, u32),
}

struct App {
    registry_state: RegistryState,
    output_state: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    conn: Connection,
    qh: QueueHandle<App>,
    loop_handle: LoopHandle<'static, App>,

    cfg: Config,
    renderer: Renderer,
    target: Option<Target>,
    bars: Vec<Bar>,
    visible: bool,

    start: Instant,
    timer: Option<RegistrationToken>,
    tray: Option<ksni::blocking::Handle<Tray>>,
    exit: bool,
}

pub fn run(cfg: Config) -> Result<()> {
    let conn = Connection::connect_to_env()
        .context("connecting to the Wayland compositor (is WAYLAND_DISPLAY set?)")?;
    let (globals, event_queue) = registry_queue_init(&conn).context("Wayland registry init")?;
    let qh: QueueHandle<App> = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).context("wl_compositor is missing")?;
    let layer_shell = LayerShell::bind(&globals, &qh)
        .context("this compositor does not implement zwlr_layer_shell_v1")?;
    let output_state = OutputState::new(&globals, &qh);
    let registry_state = RegistryState::new(&globals);

    let mut event_loop: EventLoop<'static, App> =
        EventLoop::try_new().context("creating the event loop")?;
    let loop_handle = event_loop.handle();

    let (tx, rx) = calloop::channel::channel::<Command>();
    let sender = CmdSender::new(tx);

    // Claiming the bus name is also the single-instance guard, so do it before
    // building anything expensive.
    let _dbus = crate::ipc::serve(sender.clone())?;

    let palette = crate::palette::lookup(&cfg.palette)
        .ok_or_else(|| anyhow!("unknown palette {:?}", cfg.palette))?;
    let renderer = Renderer::new(
        conn.backend().display_ptr() as *mut std::ffi::c_void,
        cfg.brightness,
        cfg.inner_fade,
        palette,
    )?;

    let mut app = App {
        registry_state,
        output_state,
        compositor,
        layer_shell,
        conn: conn.clone(),
        qh: qh.clone(),
        loop_handle: loop_handle.clone(),
        cfg,
        renderer,
        target: None,
        bars: Vec::new(),
        visible: false,
        start: Instant::now(),
        timer: None,
        tray: None,
        exit: false,
    };

    // Outputs are bound during `OutputState::new`, but their properties only
    // arrive as events, so we need a roundtrip before we can pick one.
    let mut event_queue = event_queue;
    event_queue
        .roundtrip(&mut app)
        .context("initial Wayland roundtrip")?;
    app.resolve_target()?;

    let tray = Tray {
        visible: false,
        tx: sender.clone(),
    };
    match ksni::blocking::TrayMethods::spawn(tray) {
        Ok(handle) => app.tray = Some(handle),
        Err(e) => eprintln!("ultrawide-side-saver: no system tray available ({e}); continuing"),
    }

    loop_handle
        .insert_source(rx, |event, _, app| {
            if let calloop::channel::Event::Msg(cmd) = event {
                app.handle(cmd);
            }
        })
        .map_err(|e| anyhow!("registering the control channel: {e}"))?;

    WaylandSource::new(conn.clone(), event_queue)
        .insert(loop_handle.clone())
        .map_err(|e| anyhow!("registering the Wayland source: {e}"))?;

    if app.cfg.start_visible {
        app.show();
    }

    let target_name = app
        .target
        .as_ref()
        .map(|t| t.name.clone())
        .unwrap_or_default();
    eprintln!(
        "ultrawide-side-saver: ready on {target_name} ({}x{}), bars {}px, {} fps",
        app.target.as_ref().map(|t| t.size.0).unwrap_or(0),
        app.target.as_ref().map(|t| t.size.1).unwrap_or(0),
        app.bar_width().unwrap_or(0),
        app.cfg.fps,
    );

    let signal = event_loop.get_signal();
    event_loop
        .run(None, &mut app, move |app| {
            if app.exit {
                signal.stop();
            }
            let _ = app.conn.flush();
        })
        .map_err(|e| anyhow!("event loop: {e}"))?;

    app.hide();
    Ok(())
}

impl App {
    // -- output selection ---------------------------------------------------

    fn resolve_target(&mut self) -> Result<()> {
        let wanted = self.cfg.output.trim().to_string();
        let mut best: Option<(wl_output::WlOutput, OutputInfo, (u32, u32))> = None;

        for output in self.output_state.outputs() {
            let Some(info) = self.output_state.info(&output) else {
                continue;
            };
            let Some(size) = logical_size(&info) else {
                continue;
            };
            if !wanted.is_empty() {
                let matches = info.name.as_deref() == Some(wanted.as_str())
                    || info.description.as_deref() == Some(wanted.as_str());
                if matches {
                    best = Some((output, info, size));
                    break;
                }
                continue;
            }
            // Auto: widest aspect ratio wins, which is what "the ultrawide" means.
            let aspect = size.0 as f32 / size.1 as f32;
            let better = best
                .as_ref()
                .is_none_or(|(_, _, s)| aspect > s.0 as f32 / s.1 as f32);
            if better {
                best = Some((output, info, size));
            }
        }

        let (output, info, size) = best.ok_or_else(|| {
            if wanted.is_empty() {
                anyhow!("no usable outputs found")
            } else {
                anyhow!("no output named {wanted:?}; check `kscreen-doctor -o`")
            }
        })?;

        let name = info
            .name
            .clone()
            .or(info.description.clone())
            .unwrap_or_else(|| "<unnamed>".into());
        self.target = Some(Target { output, name, size });
        Ok(())
    }

    fn bar_width(&self) -> Option<u32> {
        let t = self.target.as_ref()?;
        self.cfg.bar_width_for(t.size.0, t.size.1).ok()
    }

    // -- commands -----------------------------------------------------------

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::Show => self.show(),
            Command::Hide => self.hide(),
            Command::Toggle => {
                if self.visible {
                    self.hide()
                } else {
                    self.show()
                }
            }
            Command::Reload => self.reload(),
            Command::Quit => {
                self.hide();
                self.exit = true;
            }
        }
        let _ = self.conn.flush();
    }

    fn reload(&mut self) {
        match Config::load() {
            Ok(cfg) => {
                let was_visible = self.visible;
                self.hide();
                let palette = crate::palette::lookup(&cfg.palette).expect("validated on load");
                self.renderer.reconfigure(cfg.brightness, cfg.inner_fade, palette);
                self.cfg = cfg;
                if self.cfg.output.trim() != self.target.as_ref().map(|t| t.name.as_str()).unwrap_or("")
                    && !self.cfg.output.trim().is_empty()
                {
                    if let Err(e) = self.resolve_target() {
                        eprintln!("ultrawide-side-saver: reload: {e:#}");
                    }
                }
                if was_visible {
                    self.show();
                }
                eprintln!("ultrawide-side-saver: config reloaded");
            }
            Err(e) => eprintln!("ultrawide-side-saver: reload failed, keeping old config: {e:#}"),
        }
    }

    // -- show / hide --------------------------------------------------------

    fn show(&mut self) {
        if self.visible {
            return;
        }
        let Some(target) = self.target.as_ref() else {
            eprintln!("ultrawide-side-saver: no output to draw on");
            return;
        };
        let (ow, oh) = target.size;
        let bar_w = match self.cfg.bar_width_for(ow, oh) {
            Ok(0) => {
                eprintln!(
                    "ultrawide-side-saver: {ow}x{oh} has no room left over for {} content; nothing to draw",
                    self.cfg.content_aspect
                );
                return;
            }
            Ok(w) => w,
            Err(e) => {
                eprintln!("ultrawide-side-saver: {e:#}");
                return;
            }
        };
        let output = target.output.clone();
        let qh = self.qh.clone();

        for side in [Side::Left, Side::Right] {
            let surface = self.compositor.create_surface(&qh);

            // Empty input region: every click, scroll and hover falls through to
            // the game or browser underneath.
            match Region::new(&self.compositor) {
                Ok(region) => surface.set_input_region(Some(region.wl_region())),
                Err(e) => eprintln!("ultrawide-side-saver: could not make the bars click-through: {e}"),
            }

            let layer = self.layer_shell.create_layer_surface(
                &qh,
                surface,
                Layer::Overlay,
                Some("ultrawide-side-saver"),
                Some(&output),
            );
            let edge = match side {
                Side::Left => Anchor::LEFT,
                Side::Right => Anchor::RIGHT,
            };
            layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | edge);
            // Height 0 with both vertical anchors set means "as tall as the output".
            layer.set_size(bar_w, 0);
            // Reserve nothing either way. -1 additionally means "don't move me out
            // of anyone else's exclusive zone", i.e. draw over panels; 0 means
            // "keep me clear of them", i.e. stop short of the panel.
            layer.set_exclusive_zone(if self.cfg.cover_panels { -1 } else { 0 });
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            layer.commit();

            self.bars.push(Bar {
                side,
                layer,
                target: None,
                logical: (bar_w, oh),
                scale: 1,
            });
        }

        self.visible = true;
        self.start_timer();
        self.publish_state();
    }

    fn hide(&mut self) {
        self.stop_timer();
        if !self.bars.is_empty() {
            // Deleting the GL program needs a current context, and the context
            // needs a live surface, so tear down in this order.
            self.renderer.release_gpu();
            for mut bar in std::mem::take(&mut self.bars) {
                if let Some(t) = bar.target.take() {
                    self.renderer.egl().destroy_surface(t);
                }
                // Dropping the LayerSurface destroys it compositor-side.
            }
        }
        self.visible = false;
        self.publish_state();
        let _ = self.conn.flush();
    }

    fn publish_state(&self) {
        if let Some(tray) = self.tray.as_ref() {
            let visible = self.visible;
            tray.update(move |t| t.visible = visible);
        }
    }

    // -- frame pacing -------------------------------------------------------

    fn frame_interval(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.cfg.fps.clamp(1, 240) as f64)
    }

    fn start_timer(&mut self) {
        if self.timer.is_some() {
            return;
        }
        let res = self.loop_handle.insert_source(
            Timer::from_duration(self.frame_interval()),
            |_, _, app| {
                app.tick();
                TimeoutAction::ToDuration(app.frame_interval())
            },
        );
        match res {
            Ok(token) => self.timer = Some(token),
            Err(e) => eprintln!("ultrawide-side-saver: could not start the frame timer: {e}"),
        }
    }

    /// With no timer registered the process blocks in `poll` indefinitely, so
    /// hidden really does mean zero cost.
    fn stop_timer(&mut self) {
        if let Some(token) = self.timer.take() {
            self.loop_handle.remove(token);
        }
    }

    fn tick(&mut self) {
        for idx in 0..self.bars.len() {
            if let Err(e) = self.draw_bar(idx) {
                eprintln!("ultrawide-side-saver: draw failed: {e:#}");
            }
        }
        let _ = self.conn.flush();
    }

    // -- per-bar plumbing ---------------------------------------------------

    fn bar_index(&self, surface: &wl_surface::WlSurface) -> Option<usize> {
        self.bars.iter().position(|b| b.layer.wl_surface() == surface)
    }

    /// Create the EGL surface on first configure, or resize it afterwards.
    fn sync_bar_size(&mut self, idx: usize) {
        let (lw, lh) = self.bars[idx].logical;
        let scale = self.bars[idx].scale.max(1);
        let bw = (lw as i32 * scale).max(1);
        let bh = (lh as i32 * scale).max(1);

        if let Some(t) = self.bars[idx].target.as_mut() {
            t.resize(bw, bh);
            return;
        }
        let id = self.bars[idx].layer.wl_surface().id();
        match self.renderer.egl().create_surface(id, bw, bh) {
            Ok(t) => self.bars[idx].target = Some(t),
            Err(e) => eprintln!("ultrawide-side-saver: {e:#}"),
        }
    }

    fn placement(&self, idx: usize) -> BarPlacement {
        let bar = &self.bars[idx];
        let scale = bar.scale.max(1) as f32;
        let (ow, oh) = self.target.as_ref().map(|t| t.size).unwrap_or((1, 1));
        let (ow, oh) = (ow as f32 * scale, oh as f32 * scale);
        let bw = bar.logical.0 as f32 * scale;
        let (origin_x, inner_dir) = match bar.side {
            Side::Left => (0.0, 1.0),
            Side::Right => ((ow - bw).max(0.0), -1.0),
        };
        BarPlacement {
            origin: [origin_x, 0.0],
            output: [ow, oh],
            inner_dir,
        }
    }

    fn draw_bar(&mut self, idx: usize) -> Result<()> {
        if idx >= self.bars.len() {
            return Ok(());
        }
        let placement = self.placement(idx);
        let phase = Renderer::phase(self.start.elapsed().as_secs_f32(), self.cfg.speed);
        let App { bars, renderer, .. } = self;
        let Some(target) = bars[idx].target.as_ref() else {
            return Ok(());
        };
        renderer.draw(target, placement, phase)
    }
}

// ---------------------------------------------------------------------------
// Wayland protocol handlers
// ---------------------------------------------------------------------------

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        let Some(idx) = self.bar_index(surface) else {
            return;
        };
        let factor = new_factor.max(1);
        if self.bars[idx].scale == factor {
            return;
        }
        self.bars[idx].scale = factor;
        self.bars[idx].layer.wl_surface().set_buffer_scale(factor);
        self.sync_bar_size(idx);
        let _ = self.draw_bar(idx);
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        // Pacing comes from the frame timer, not from frame callbacks: rendering
        // well below the refresh rate is the entire point.
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if self.target.as_ref().map(|t| &t.output) != Some(&output) {
            return;
        }
        let Some(size) = self.output_state.info(&output).and_then(|i| logical_size(&i)) else {
            return;
        };
        if self.target.as_ref().map(|t| t.size) == Some(size) {
            return;
        }
        // Mode or rotation changed: rebuild the bars at the new width.
        if let Some(t) = self.target.as_mut() {
            t.size = size;
        }
        if self.visible {
            self.hide();
            self.show();
        }
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if self.target.as_ref().map(|t| &t.output) == Some(&output) {
            self.hide();
            self.target = None;
            // Re-pick once the display comes back (e.g. after a DPMS cycle).
            if let Err(e) = self.resolve_target() {
                eprintln!("ultrawide-side-saver: output went away: {e:#}");
            }
        }
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, layer: &LayerSurface) {
        if self.bars.iter().any(|b| &b.layer == layer) {
            self.hide();
        }
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let Some(idx) = self.bars.iter().position(|b| &b.layer == layer) else {
            return;
        };
        let (w, h) = configure.new_size;
        if w == 0 || h == 0 {
            return;
        }
        self.bars[idx].logical = (w, h);
        self.sync_bar_size(idx);
        if let Err(e) = self.draw_bar(idx) {
            eprintln!("ultrawide-side-saver: draw failed: {e:#}");
        }
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

delegate_registry!(App);
delegate_dispatch2!(App);

// ---------------------------------------------------------------------------
// `outputs` subcommand: enumerate outputs without starting the daemon
// ---------------------------------------------------------------------------

struct Lister {
    registry_state: RegistryState,
    output_state: OutputState,
}

impl OutputHandler for Lister {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ProvidesRegistryState for Lister {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

delegate_registry!(Lister);
delegate_dispatch2!(Lister);

/// `(connector name, logical width, logical height)` for each connected output.
pub fn list_outputs() -> Result<Vec<(String, u32, u32)>> {
    let conn = Connection::connect_to_env().context("connecting to the Wayland compositor")?;
    let (globals, mut queue) = registry_queue_init::<Lister>(&conn).context("registry init")?;
    let qh = queue.handle();
    let mut lister = Lister {
        output_state: OutputState::new(&globals, &qh),
        registry_state: RegistryState::new(&globals),
    };
    queue.roundtrip(&mut lister).context("Wayland roundtrip")?;

    let mut out = Vec::new();
    for output in lister.output_state.outputs() {
        let Some(info) = lister.output_state.info(&output) else {
            continue;
        };
        let Some((w, h)) = logical_size(&info) else {
            continue;
        };
        let name = info
            .name
            .clone()
            .or(info.description.clone())
            .unwrap_or_else(|| "<unnamed>".into());
        out.push((name, w, h));
    }
    Ok(out)
}

/// Logical size of an output, preferring xdg_output and falling back to the
/// current mode with the output transform applied.
fn logical_size(info: &OutputInfo) -> Option<(u32, u32)> {
    if let Some((w, h)) = info.logical_size {
        if w > 0 && h > 0 {
            return Some((w as u32, h as u32));
        }
    }
    let mode = info.modes.iter().find(|m| m.current)?;
    let (w, h) = mode.dimensions;
    let rotated = matches!(
        info.transform,
        wl_output::Transform::_90
            | wl_output::Transform::_270
            | wl_output::Transform::Flipped90
            | wl_output::Transform::Flipped270
    );
    let (w, h) = if rotated { (h, w) } else { (w, h) };
    (w > 0 && h > 0).then_some((w as u32, h as u32))
}
