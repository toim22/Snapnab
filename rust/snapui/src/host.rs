//! The window host: every Snapnab window is a winit window drawn by egui on the GPU (wgpu). eframe would do this for
//! one app window, but Snapnab needs things eframe hides: its own window classes ("SnapnabUI", the selection overlay's
//! "SnapnabSelectWindowClass"…, which the Editor and the e2e suite look for), raw Win32 messages (the command line
//! another instance forwards, tray clicks, hotkeys) and many windows at once (overlays, pins, the recording HUD).
//!
//! How a frame happens (immediate mode): when a window needs drawing, its egui `Context` runs the app's `ui` function
//! with the input gathered since the last frame; egui returns shapes, which become triangles (tessellation) that the
//! window's `egui_wgpu::Renderer` draws into the window's surface. All windows share one GPU device; each has its own
//! Context, input state and renderer. egui asks for the next frame itself (an animation, a hover, a timer) through
//! the repaint callback, which wakes the event loop.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::windows::WindowAttributesExtWindows;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowAttributes, WindowId, WindowLevel};

pub type WinKey = WindowId;

/// A Win32 message caught before winit sees it.
#[derive(Clone, Debug)]
pub struct RawMsg {
    pub hwnd: isize,
    pub msg: u32,
    pub wparam: usize,
    pub lparam: isize,
    /// data that only lives during the call (the text of WM_COPYDATA), copied out by the intercept function
    pub text: Option<String>,
}

/// What an intercept function does with a message.
pub enum Caught {
    /// winit handles it as usual
    Pass,
    /// answered here, winit never sees it
    Handled(isize),
    /// answered here and handed to the app's `message` (after the call returns, on the event loop)
    Queue(RawMsg, isize),
}

/// Runs inside the window procedure, before winit: it must not touch the app (which may be busy), only decide.
pub type Intercept = fn(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> Caught;

#[derive(Clone)]
pub struct WindowSpec {
    /// the Win32 window class
    pub class: &'static str,
    pub title: String,
    /// inner size in points (pixels at 100 %), or in physical pixels with `physical_size`
    pub size: (f32, f32),
    /// `size` is in physical pixels (a window that must cover an exact screen area)
    pub physical_size: bool,
    /// outer top-left corner in physical pixels; None = where Windows puts it
    pub position: Option<(i32, i32)>,
    pub visible: bool,
    pub decorations: bool,
    pub resizable: bool,
    pub topmost: bool,
    pub skip_taskbar: bool,
    pub intercept: Option<Intercept>,
    pub clear_color: [f32; 4],
}

impl Default for WindowSpec {
    fn default() -> Self {
        WindowSpec {
            class: "SnapnabWindow",
            title: "Snapnab".into(),
            size: (640.0, 400.0),
            physical_size: false,
            position: None,
            visible: true,
            decorations: true,
            resizable: true,
            topmost: false,
            skip_taskbar: false,
            intercept: None,
            clear_color: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

/// The app behind the windows. Every call gets a `Ctl` to create, show and close windows.
pub trait App: 'static {
    /// The event loop is ready: create the first windows.
    fn start(&mut self, ctl: &mut Ctl);
    /// Draws one frame of a window.
    fn ui(&mut self, ctl: &mut Ctl, win: WinKey, ui: &mut egui::Ui);
    /// A message an intercept function queued.
    fn message(&mut self, _ctl: &mut Ctl, _msg: RawMsg) {}
    /// The window's close button (or WM_CLOSE): closes it unless the app overrides this.
    fn close_requested(&mut self, ctl: &mut Ctl, win: WinKey) {
        ctl.close(win);
    }
    /// Something another thread sent through a `Waker`.
    fn event(&mut self, _ctl: &mut Ctl, _ev: Box<dyn Any + Send>) {}
}

pub enum HostEvent {
    AccessKit(accesskit_winit::Event),
    Repaint { win: WinKey, delay: Duration },
    Raw(RawMsg),
    App(Box<dyn Any + Send>),
}

impl From<accesskit_winit::Event> for HostEvent {
    fn from(e: accesskit_winit::Event) -> Self {
        HostEvent::AccessKit(e)
    }
}

/// Hands values from other threads to `App::event`.
#[derive(Clone)]
pub struct Waker(EventLoopProxy<HostEvent>);

impl Waker {
    pub fn send(&self, ev: impl Any + Send) {
        let _ = self.0.send_event(HostEvent::App(Box::new(ev)));
    }
}

struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

struct Win {
    window: Arc<Window>,
    hwnd: isize,
    ctx: egui::Context,
    state: egui_winit::State,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: egui_wgpu::Renderer,
    clear: [f32; 4],
    repaint_at: Option<Instant>,
    subclass: Option<Box<Sub>>,
}

struct Sub {
    intercept: Intercept,
    proxy: EventLoopProxy<HostEvent>,
}

/// What the app may do from its callbacks.
pub struct Ctl<'a> {
    el: &'a ActiveEventLoop,
    wins: &'a mut HashMap<WinKey, Win>,
    gpu: &'a mut Option<Gpu>,
    proxy: &'a EventLoopProxy<HostEvent>,
    /// windows closed while they were drawing their own frame (out of `wins` for that time)
    closed: &'a mut Vec<WinKey>,
    /// the window drawing its frame now (out of `wins` meanwhile): its key, window and handle
    current: Option<(WinKey, Arc<Window>, isize)>,
}

/// Takes the subclass off before its data goes away with the window.
fn unsubclass(w: &Win) {
    if w.subclass.is_some() {
        unsafe {
            let _ = RemoveWindowSubclass(HWND(w.hwnd as _), Some(subclass_proc), 1);
        }
    }
}

fn hwnd_of(w: &Window) -> isize {
    match w.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
        _ => 0,
    }
}

unsafe extern "system" fn subclass_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, data: usize) -> LRESULT {
    let sub = unsafe { &*(data as *const Sub) };
    match (sub.intercept)(hwnd.0 as isize, msg, wp.0, lp.0) {
        Caught::Pass => unsafe { DefSubclassProc(hwnd, msg, wp, lp) },
        Caught::Handled(r) => LRESULT(r),
        Caught::Queue(m, r) => {
            let _ = sub.proxy.send_event(HostEvent::Raw(m));
            LRESULT(r)
        }
    }
}

impl Ctl<'_> {
    /// Creates a window (hidden first, so the accessibility tree is ready before it shows).
    pub fn create(&mut self, spec: &WindowSpec) -> Option<WinKey> {
        self.create_with(spec, |_| {})
    }

    /// `create`, with `early` called with the window handle as soon as the window exists, before the GPU work that
    /// takes a few hundred milliseconds (the capture app registers its global hotkeys there).
    pub fn create_with(&mut self, spec: &WindowSpec, early: impl FnOnce(isize)) -> Option<WinKey> {
        let mut attrs: WindowAttributes = Window::default_attributes()
            .with_title(spec.title.clone())
            .with_visible(false)
            .with_decorations(spec.decorations)
            .with_resizable(spec.resizable)
            .with_class_name(spec.class)
            .with_skip_taskbar(spec.skip_taskbar)
            .with_drag_and_drop(false);
        attrs = if spec.physical_size {
            attrs.with_inner_size(PhysicalSize::new(spec.size.0 as u32, spec.size.1 as u32))
        } else {
            attrs.with_inner_size(LogicalSize::new(spec.size.0, spec.size.1))
        };
        if spec.topmost {
            attrs = attrs.with_window_level(WindowLevel::AlwaysOnTop);
        }
        if let Some((x, y)) = spec.position {
            attrs = attrs.with_position(PhysicalPosition::new(x, y));
        }
        let window = Arc::new(self.el.create_window(attrs).ok()?);
        let key = window.id();
        // the subclass goes in first: another program may already be sending to the window (a second instance
        // forwarding its command line finds it by class as soon as it exists)
        let hwnd = hwnd_of(&window);
        let subclass = spec.intercept.map(|intercept| {
            let sub = Box::new(Sub { intercept, proxy: self.proxy.clone() });
            unsafe {
                let _ = SetWindowSubclass(HWND(hwnd as _), Some(subclass_proc), 1, &*sub as *const Sub as usize);
            }
            sub
        });
        early(hwnd);
        if self.gpu.is_none() {
            *self.gpu = Some(new_gpu()?);
        }
        let gpu = self.gpu.as_ref()?;
        let surface = gpu.instance.create_surface(window.clone()).ok()?;
        let format = egui_wgpu::preferred_framebuffer_format(&surface.get_capabilities(&gpu.adapter).formats).ok()?;
        let size = window.inner_size();
        let mut config = surface.get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))?;
        config.format = format;
        config.view_formats = vec![format];
        config.present_mode = wgpu::PresentMode::AutoVsync;
        config.desired_maximum_frame_latency = 2;
        surface.configure(&gpu.device, &config);
        let renderer = egui_wgpu::Renderer::new(&gpu.device, format, egui_wgpu::RendererOptions::default());

        let ctx = egui::Context::default();
        crate::install_fonts(&ctx);
        let proxy = self.proxy.clone();
        ctx.set_request_repaint_callback(move |info| {
            let _ = proxy.send_event(HostEvent::Repaint { win: key, delay: info.delay });
        });
        let max_side = gpu.device.limits().max_texture_dimension_2d as usize;
        let mut state = egui_winit::State::new(ctx.clone(), egui::ViewportId::ROOT, &window,
                                               Some(window.scale_factor() as f32), window.theme(), Some(max_side));
        state.init_accesskit(self.el, &window, self.proxy.clone());
        self.wins.insert(key, Win {
            window: window.clone(),
            hwnd,
            ctx,
            state,
            surface,
            config,
            renderer,
            clear: spec.clear_color,
            repaint_at: Some(Instant::now()),
            subclass,
        });
        if spec.visible {
            window.set_visible(true);
        }
        window.request_redraw();
        Some(key)
    }

    pub fn close(&mut self, win: WinKey) {
        match self.wins.remove(&win) {
            Some(w) => unsubclass(&w),
            None => self.closed.push(win),
        }
    }

    fn drawing(&self, win: WinKey) -> Option<&(WinKey, Arc<Window>, isize)> {
        self.current.as_ref().filter(|c| c.0 == win && !self.closed.contains(&win))
    }

    pub fn exists(&self, win: WinKey) -> bool {
        self.wins.contains_key(&win) || self.drawing(win).is_some()
    }

    pub fn hwnd(&self, win: WinKey) -> isize {
        self.wins.get(&win).map(|w| w.hwnd).or_else(|| self.drawing(win).map(|c| c.2)).unwrap_or(0)
    }

    pub fn window(&self, win: WinKey) -> Option<Arc<Window>> {
        self.wins.get(&win).map(|w| w.window.clone()).or_else(|| self.drawing(win).map(|c| c.1.clone()))
    }

    pub fn repaint(&self, win: WinKey) {
        if let Some(w) = self.window(win) {
            w.request_redraw();
        }
    }

    pub fn waker(&self) -> Waker {
        Waker(self.proxy.clone())
    }

    /// Ends the event loop (every window closes).
    pub fn exit(&mut self) {
        self.el.exit();
    }
}

/// The GPU device every window shares: Direct3D 12 first (quick to start, and WARP draws it on machines without a
/// usable GPU), then the other backends. The low-power adapter is the one that drives the screens on laptops.
fn new_gpu() -> Option<Gpu> {
    let attempt = |backends: wgpu::Backends| {
        let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
        setup.instance_descriptor.backends = backends;
        setup.power_preference = wgpu::PowerPreference::LowPower;
        setup.device_descriptor = Arc::new(|a| wgpu::DeviceDescriptor {
            label: Some("snapui"),
            // a frozen screen can be as large as the adapter allows (several 4K monitors side by side)
            required_limits: wgpu::Limits { max_texture_dimension_2d: a.limits().max_texture_dimension_2d, ..Default::default() },
            ..Default::default()
        });
        let config = egui_wgpu::WgpuConfiguration { wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(setup), ..Default::default() };
        pollster::block_on(async {
            let instance = config.wgpu_setup.new_instance().await;
            let rs = egui_wgpu::RenderState::create(&config, &instance, None, egui_wgpu::RendererOptions::default())
                .await
                .map_err(|e| snapcore::log::write(&format!("GPU ({backends:?}): {e}")))
                .ok()?;
            let info = rs.adapter.get_info();
            snapcore::log::write(&format!("GPU: {} ({:?})", info.name, info.backend));
            Some(Gpu { instance, adapter: rs.adapter, device: rs.device, queue: rs.queue })
        })
    };
    attempt(wgpu::Backends::DX12).or_else(|| attempt(wgpu::Backends::PRIMARY | wgpu::Backends::GL))
}

struct Host<A: App> {
    app: A,
    wins: HashMap<WinKey, Win>,
    gpu: Option<Gpu>,
    proxy: EventLoopProxy<HostEvent>,
    started: bool,
    closed: Vec<WinKey>,
}

macro_rules! ctl {
    ($s:ident, $el:ident) => {
        &mut Ctl { el: $el, wins: &mut $s.wins, gpu: &mut $s.gpu, proxy: &$s.proxy, closed: &mut $s.closed, current: None }
    };
    ($s:ident, $el:ident, $cur:expr) => {
        &mut Ctl { el: $el, wins: &mut $s.wins, gpu: &mut $s.gpu, proxy: &$s.proxy, closed: &mut $s.closed, current: $cur }
    };
}

impl<A: App> Host<A> {
    fn frame(&mut self, el: &ActiveEventLoop, key: WinKey) {
        let Some(mut w) = self.wins.remove(&key) else { return };
        w.repaint_at = None;
        let input = w.state.take_egui_input(&w.window);
        let ctx = w.ctx.clone();
        let cur = Some((key, w.window.clone(), w.hwnd));
        let out = ctx.run_ui(input, |ui| self.app.ui(ctl!(self, el, cur.clone()), key, ui));
        // the app may have closed this window during its own frame
        if let Some(i) = self.closed.iter().position(|&k| k == key) {
            self.closed.swap_remove(i);
            unsubclass(&w);
            return;
        }
        w.state.handle_platform_output(&w.window, out.platform_output);
        let ppp = out.pixels_per_point;
        let prims = ctx.tessellate(out.shapes, ppp);
        if let Some(gpu) = &self.gpu {
            paint(gpu, &mut w, &prims, out.textures_delta, ppp);
        }
        if let Some(vo) = out.viewport_output.get(&egui::ViewportId::ROOT) {
            schedule(&mut w, vo.repaint_delay);
        }
        self.wins.insert(key, w);
    }
}

fn schedule(w: &mut Win, delay: Duration) {
    if delay == Duration::MAX {
        return;
    }
    let at = Instant::now() + delay;
    if delay.is_zero() {
        w.window.request_redraw();
    } else if w.repaint_at.is_none_or(|t| at < t) {
        w.repaint_at = Some(at);
    }
}

fn paint(gpu: &Gpu, w: &mut Win, prims: &[egui::ClippedPrimitive], mut delta: egui::TexturesDelta, ppp: f32) {
    for (id, images) in delta.set.drain() {
        for image in &images {
            w.renderer.update_texture(&gpu.device, &gpu.queue, id, image);
        }
    }
    let sd = egui_wgpu::ScreenDescriptor { size_in_pixels: [w.config.width, w.config.height], pixels_per_point: ppp };
    let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("snapui") });
    let user = w.renderer.update_buffers(&gpu.device, &gpu.queue, &mut enc, prims, &sd);
    let frame = match w.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(f) => Some(f),
        wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
            w.surface.configure(&gpu.device, &w.config);
            Some(f)
        }
        wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
            w.surface.configure(&gpu.device, &w.config);
            w.window.request_redraw();
            None
        }
        _ => None,
    };
    if let Some(frame) = frame {
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let [r, g, b, a] = w.clear.map(f64::from);
            let pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("snapui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }), store: wgpu::StoreOp::Store },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            w.renderer.render(&mut pass.forget_lifetime(), prims, &sd);
        }
        gpu.queue.submit(user.into_iter().chain([enc.finish()]));
        w.window.pre_present_notify();
        gpu.queue.present(frame);
    } else {
        gpu.queue.submit(user.into_iter().chain([enc.finish()]));
    }
    for id in &delta.free {
        w.renderer.free_texture(id);
    }
}

impl<A: App> ApplicationHandler<HostEvent> for Host<A> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if !self.started {
            self.started = true;
            self.app.start(ctl!(self, el));
        }
    }

    fn new_events(&mut self, _el: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause {
            let now = Instant::now();
            for w in self.wins.values_mut() {
                if w.repaint_at.is_some_and(|t| t <= now) {
                    w.repaint_at = None;
                    w.window.request_redraw();
                }
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(w) = self.wins.get_mut(&id) else { return };
        match &event {
            WindowEvent::RedrawRequested => {
                self.frame(el, id);
                return;
            }
            WindowEvent::CloseRequested => {
                self.app.close_requested(ctl!(self, el), id);
                return;
            }
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 && let Some(gpu) = &self.gpu {
                    w.config.width = size.width;
                    w.config.height = size.height;
                    w.surface.configure(&gpu.device, &w.config);
                }
            }
            _ => {}
        }
        let w = self.wins.get_mut(&id).unwrap();
        let r = w.state.on_window_event(&w.window, &event);
        if r.repaint {
            w.window.request_redraw();
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: HostEvent) {
        match event {
            HostEvent::Repaint { win, delay } => {
                if let Some(w) = self.wins.get_mut(&win) {
                    schedule(w, delay);
                }
            }
            HostEvent::AccessKit(e) => {
                let Some(w) = self.wins.get_mut(&e.window_id) else { return };
                match e.window_event {
                    accesskit_winit::WindowEvent::InitialTreeRequested => {
                        w.ctx.enable_accesskit();
                        w.window.request_redraw();
                    }
                    accesskit_winit::WindowEvent::ActionRequested(req) => {
                        w.state.on_accesskit_action_request(req);
                        w.window.request_redraw();
                    }
                    accesskit_winit::WindowEvent::AccessibilityDeactivated => w.ctx.disable_accesskit(),
                }
            }
            HostEvent::Raw(m) => self.app.message(ctl!(self, el), m),
            HostEvent::App(ev) => self.app.event(ctl!(self, el), ev),
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        match self.wins.values().filter_map(|w| w.repaint_at).min() {
            Some(t) => el.set_control_flow(ControlFlow::WaitUntil(t)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        for (_, w) in self.wins.drain() {
            unsubclass(&w);
        }
    }
}

static PROXY: OnceLock<EventLoopProxy<HostEvent>> = OnceLock::new();

/// A waker before the first window exists (for threads started early), once `run` began.
pub fn waker() -> Option<Waker> {
    PROXY.get().cloned().map(Waker)
}

/// Runs the app until `Ctl::exit`. `before` gets a waker for threads the app starts before the loop does.
pub fn run(app: impl App, before: impl FnOnce(Waker)) -> Result<(), String> {
    let el = EventLoop::<HostEvent>::with_user_event().build().map_err(|e| e.to_string())?;
    let proxy = el.create_proxy();
    let _ = PROXY.set(proxy.clone());
    before(Waker(proxy.clone()));
    let mut host = Host { app, wins: HashMap::new(), gpu: None, proxy, started: false, closed: Vec::new() };
    el.run_app(&mut host).map_err(|e| e.to_string())
}
