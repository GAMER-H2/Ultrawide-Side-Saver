//! EGL + OpenGL ES 3.0 renderer.
//!
//! One EGL context is shared by both bars; each bar owns an EGLSurface backed by
//! a `wl_egl_window`. Drawing is a single fullscreen triangle with no vertex data
//! and no textures, so a frame costs one draw call and a handful of uniforms.

use anyhow::{anyhow, Context, Result};
use glow::HasContext;
use khronos_egl as egl;
use std::ffi::c_void;
use wayland_client::backend::ObjectId;

use crate::palette::Palette;

/// EGL_OPENGL_ES3_BIT. Not exposed as a constant by khronos-egl for the base API.
const OPENGL_ES3_BIT: egl::Int = 0x0040;

/// Seconds for one full loop of the animation at `speed = 1.0`. The shader's time
/// terms are all integer multiples of the phase, so wrapping here is seamless.
const LOOP_PERIOD_SECS: f32 = 600.0;
const TAU: f32 = std::f32::consts::TAU;

type EglInstance = egl::Instance<egl::Static>;

pub struct Egl {
    instance: EglInstance,
    display: egl::Display,
    config: egl::Config,
    context: egl::Context,
}

impl Egl {
    /// `wl_display` must be the live `wl_display` pointer of the connection that
    /// owns every surface later passed to [`Egl::create_surface`].
    pub fn new(wl_display: *mut c_void) -> Result<Self> {
        let instance = EglInstance::new(egl::Static);

        let display = unsafe { instance.get_display(wl_display) }
            .ok_or_else(|| anyhow!("eglGetDisplay failed for the Wayland display"))?;
        instance
            .initialize(display)
            .context("eglInitialize failed")?;
        instance
            .bind_api(egl::OPENGL_ES_API)
            .context("eglBindAPI(OpenGL ES) failed")?;

        let config_attrs = [
            egl::SURFACE_TYPE,
            egl::WINDOW_BIT,
            egl::RENDERABLE_TYPE,
            OPENGL_ES3_BIT,
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            // Alpha is required: the inner fade blends into whatever is behind us.
            egl::ALPHA_SIZE,
            8,
            egl::NONE,
        ];
        let config = instance
            .choose_first_config(display, &config_attrs)
            .context("eglChooseConfig failed")?
            .ok_or_else(|| anyhow!("no EGL config with an alpha channel and GLES3 support"))?;

        let ctx_attrs = [
            egl::CONTEXT_MAJOR_VERSION,
            3,
            egl::CONTEXT_MINOR_VERSION,
            0,
            egl::NONE,
        ];
        let context = instance
            .create_context(display, config, None, &ctx_attrs)
            .context("eglCreateContext failed")?;

        Ok(Self {
            instance,
            display,
            config,
            context,
        })
    }

    /// Build the EGL/GL surface pair for one bar.
    pub fn create_surface(&self, surface_id: ObjectId, w: i32, h: i32) -> Result<EglTarget> {
        let wl_egl = wayland_egl::WlEglSurface::new(surface_id, w, h)
            .map_err(|e| anyhow!("wl_egl_window_create failed: {e}"))?;
        let egl_surface = unsafe {
            self.instance.create_window_surface(
                self.display,
                self.config,
                wl_egl.ptr() as egl::NativeWindowType,
                None,
            )
        }
        .context("eglCreateWindowSurface failed")?;
        Ok(EglTarget {
            wl_egl,
            surface: egl_surface,
            width: w,
            height: h,
        })
    }

    fn make_current(&self, target: &EglTarget) -> Result<()> {
        self.instance
            .make_current(
                self.display,
                Some(target.surface),
                Some(target.surface),
                Some(self.context),
            )
            .context("eglMakeCurrent failed")
    }

    pub fn release_current(&self) {
        let _ = self
            .instance
            .make_current(self.display, None, None, None);
    }

    pub fn destroy_surface(&self, target: EglTarget) {
        let _ = self.instance.destroy_surface(self.display, target.surface);
        // `target.wl_egl` is dropped here, after the EGLSurface that referenced it.
    }

    /// Load GL entry points. Requires a current context, so call this after the
    /// first [`Egl::make_current`].
    fn load_gl(&self) -> glow::Context {
        unsafe {
            glow::Context::from_loader_function(|name| {
                self.instance
                    .get_proc_address(name)
                    .map(|p| p as *const c_void)
                    .unwrap_or(std::ptr::null())
            })
        }
    }
}

impl Drop for Egl {
    fn drop(&mut self) {
        self.release_current();
        let _ = self.instance.destroy_context(self.display, self.context);
        let _ = self.instance.terminate(self.display);
    }
}

pub struct EglTarget {
    #[allow(dead_code)] // Kept alive for as long as the EGLSurface references it.
    wl_egl: wayland_egl::WlEglSurface,
    surface: egl::Surface,
    width: i32,
    height: i32,
}

impl EglTarget {
    pub fn resize(&mut self, w: i32, h: i32) {
        if (w, h) != (self.width, self.height) {
            self.wl_egl.resize(w, h, 0, 0);
            self.width = w;
            self.height = h;
        }
    }
}

/// Where a bar sits within its output, and which of its edges faces the content.
#[derive(Clone, Copy, Debug)]
pub struct BarPlacement {
    /// Bar origin (bottom-left) in output pixels.
    pub origin: [f32; 2],
    /// Full output size in pixels.
    pub output: [f32; 2],
    /// +1.0 if the content-facing edge is this bar's right edge.
    pub inner_dir: f32,
}

/// The compiled program plus its uniform locations.
pub struct Gpu {
    gl: glow::Context,
    program: glow::Program,
    u_res: Option<glow::UniformLocation>,
    u_origin: Option<glow::UniformLocation>,
    u_output: Option<glow::UniformLocation>,
    u_phase: Option<glow::UniformLocation>,
    u_brightness: Option<glow::UniformLocation>,
    u_fade: Option<glow::UniformLocation>,
    u_inner_dir: Option<glow::UniformLocation>,
    u_c: [Option<glow::UniformLocation>; 3],
}

impl Gpu {
    fn new(gl: glow::Context) -> Result<Self> {
        unsafe {
            let program = compile(
                &gl,
                include_str!("../shaders/side.vert"),
                include_str!("../shaders/side.frag"),
            )?;
            let u = |n: &str| gl.get_uniform_location(program, n);
            let gpu = Self {
                u_res: u("u_res"),
                u_origin: u("u_origin"),
                u_output: u("u_output"),
                u_phase: u("u_phase"),
                u_brightness: u("u_brightness"),
                u_fade: u("u_fade"),
                u_inner_dir: u("u_inner_dir"),
                u_c: [u("u_c0"), u("u_c1"), u("u_c2")],
                program,
                gl,
            };
            gpu.gl.use_program(Some(gpu.program));
            gpu.gl.disable(glow::DEPTH_TEST);
            gpu.gl.disable(glow::BLEND);
            gpu.gl.disable(glow::CULL_FACE);
            Ok(gpu)
        }
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe { self.gl.delete_program(self.program) };
    }
}

unsafe fn compile(gl: &glow::Context, vert: &str, frag: &str) -> Result<glow::Program> {
    let program = gl
        .create_program()
        .map_err(|e| anyhow!("glCreateProgram: {e}"))?;
    let mut shaders = Vec::new();
    for (kind, src) in [(glow::VERTEX_SHADER, vert), (glow::FRAGMENT_SHADER, frag)] {
        let shader = gl
            .create_shader(kind)
            .map_err(|e| anyhow!("glCreateShader: {e}"))?;
        gl.shader_source(shader, src);
        gl.compile_shader(shader);
        if !gl.get_shader_compile_status(shader) {
            return Err(anyhow!(
                "shader compile failed: {}",
                gl.get_shader_info_log(shader)
            ));
        }
        gl.attach_shader(program, shader);
        shaders.push(shader);
    }
    gl.link_program(program);
    if !gl.get_program_link_status(program) {
        return Err(anyhow!("program link failed: {}", gl.get_program_info_log(program)));
    }
    for shader in shaders {
        gl.detach_shader(program, shader);
        gl.delete_shader(shader);
    }
    Ok(program)
}

/// Owns the EGL context and the lazily-compiled program, and draws bars.
pub struct Renderer {
    egl: Egl,
    gpu: Option<Gpu>,
    brightness: f32,
    fade: f32,
    stops: [[f32; 3]; 3],
}

impl Renderer {
    pub fn new(wl_display: *mut c_void, brightness: f32, fade: f32, palette: &Palette) -> Result<Self> {
        Ok(Self {
            egl: Egl::new(wl_display)?,
            gpu: None,
            brightness,
            fade,
            stops: palette.stops,
        })
    }

    pub fn egl(&self) -> &Egl {
        &self.egl
    }

    /// Apply new appearance settings. Uniforms are pushed every frame, so this
    /// takes effect on the next draw without touching the compiled program.
    pub fn reconfigure(&mut self, brightness: f32, fade: f32, palette: &Palette) {
        self.brightness = brightness;
        self.fade = fade;
        self.stops = palette.stops;
    }

    /// Convert wall-clock seconds since start into the shader's wrapped phase.
    pub fn phase(elapsed_secs: f32, speed: f32) -> f32 {
        let period = LOOP_PERIOD_SECS / speed.max(0.001);
        (elapsed_secs % period) / period * TAU
    }

    pub fn draw(&mut self, target: &EglTarget, placement: BarPlacement, phase: f32) -> Result<()> {
        self.egl.make_current(target)?;

        if self.gpu.is_none() {
            let gl = self.egl.load_gl();
            self.gpu = Some(Gpu::new(gl)?);
            // Never block in swap: the frame timer is what paces us, and blocking
            // here would stall the Wayland event loop behind the display refresh.
            let _ = self.egl.instance.swap_interval(self.egl.display, 0);
        }
        let gpu = self.gpu.as_ref().expect("just initialised");

        unsafe {
            let gl = &gpu.gl;
            gl.viewport(0, 0, target.width, target.height);
            gl.use_program(Some(gpu.program));
            gl.uniform_2_f32(gpu.u_res.as_ref(), target.width as f32, target.height as f32);
            gl.uniform_2_f32(gpu.u_origin.as_ref(), placement.origin[0], placement.origin[1]);
            gl.uniform_2_f32(gpu.u_output.as_ref(), placement.output[0], placement.output[1]);
            gl.uniform_1_f32(gpu.u_phase.as_ref(), phase);
            gl.uniform_1_f32(gpu.u_brightness.as_ref(), self.brightness);
            gl.uniform_1_f32(gpu.u_fade.as_ref(), self.fade);
            gl.uniform_1_f32(gpu.u_inner_dir.as_ref(), placement.inner_dir);
            for (loc, c) in gpu.u_c.iter().zip(self.stops.iter()) {
                gl.uniform_3_f32(loc.as_ref(), c[0], c[1], c[2]);
            }
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
        }

        self.egl
            .instance
            .swap_buffers(self.egl.display, target.surface)
            .context("eglSwapBuffers failed")?;
        Ok(())
    }

    /// Drop the GL program. Called when the bars are hidden so nothing GPU-side
    /// lingers; it is recompiled on the next show.
    pub fn release_gpu(&mut self) {
        self.gpu = None;
        self.egl.release_current();
    }
}
