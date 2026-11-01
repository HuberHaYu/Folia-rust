// The wall's ground (theme background with screen-fixed radial glows) and veil (vignette and grain over the tiles).

use iced::widget::shader::{self, Primitive as ShaderPrimitive};
use iced::{Rectangle, mouse};

use crate::pipeline::{Fullscreen, Slot};

const SHADER: &str = concat!(include_str!("../shaders/fullscreen.wgsl"), include_str!("../shaders/wash.wgsl"));

/// One `radial-gradient(circle at center, rgba, transparent reach)`; colors are sRGB in 0..=1.
#[derive(Debug, Clone, Copy, Default)]
pub struct Glow {
    /// Fractions of the widget, Y downward.
    pub center: [f32; 2],
    /// Where the glow has faded out, as a fraction of the distance to the farthest corner.
    pub reach: f32,
    pub rgba: [f32; 4],
}

/// Opaque `base` with `glows[0]` painted over `glows[1]`, the order CSS stacks backgrounds in.
#[derive(Debug, Clone, Copy)]
pub struct Wash {
    pub base: [f32; 3],
    pub glows: [Glow; 2],
}

/// Black edge falloff reaching `vignette` at the corners, and a static grain of strength `grain`.
#[derive(Debug, Clone, Copy)]
pub struct Veil {
    pub vignette: f32,
    pub grain: f32,
}

impl<Message> shader::Program<Message> for Wash {
    type State = ();
    type Primitive = WashPass;

    fn draw(&self, _state: &Self::State, _cursor: mouse::Cursor, bounds: Rectangle) -> Self::Primitive {
        let [r, g, b] = self.base;
        let mut uniforms = Uniforms::sized(bounds);
        uniforms.base = [r, g, b, 1.0];
        uniforms.glows =
            self.glows.map(|glow| GlowUniform { center: glow.center, reach: glow.reach, _pad: 0.0, color: glow.rgba });
        WashPass(uniforms, Slot::default())
    }
}

impl<Message> shader::Program<Message> for Veil {
    type State = ();
    type Primitive = VeilPass;

    fn draw(&self, _state: &Self::State, _cursor: mouse::Cursor, bounds: Rectangle) -> Self::Primitive {
        let mut uniforms = Uniforms::sized(bounds);
        uniforms.vignette = self.vignette;
        uniforms.grain = self.grain;
        VeilPass(uniforms, Slot::default())
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct GlowUniform {
    center: [f32; 2],
    reach: f32,
    _pad: f32,
    color: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    resolution: [f32; 2],
    encode: f32,
    vignette: f32,
    grain: f32,
    _pad: [f32; 3],
    base: [f32; 4],
    glows: [GlowUniform; 2],
}

impl Uniforms {
    fn sized(bounds: Rectangle) -> Self {
        Self { resolution: [bounds.width.max(1.0), bounds.height.max(1.0)], ..Self::default() }
    }
}

#[derive(Debug)]
pub struct WashPass(Uniforms, Slot);

#[derive(Debug)]
pub struct VeilPass(Uniforms, Slot);

/// A full-screen pass and whether its target stores sRGB, so the shader must hand back linear color.
pub struct Gpu {
    fullscreen: Fullscreen,
    srgb_target: bool,
}

impl Gpu {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, fragment: &str) -> Self {
        let fullscreen = Fullscreen::new(device, format, fragment, SHADER, fragment, size_of::<Uniforms>());
        Self { fullscreen, srgb_target: format.is_srgb() }
    }

    fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, slot: &Slot, uniforms: Uniforms) {
        let uniforms = Uniforms { encode: if self.srgb_target { 1.0 } else { 0.0 }, ..uniforms };
        self.fullscreen.write(device, queue, slot, bytemuck::bytes_of(&uniforms));
    }
}

pub struct WashGpu(Gpu);

pub struct VeilGpu(Gpu);

impl shader::Pipeline for WashGpu {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self(Gpu::new(device, format, "fs_wash"))
    }

    fn trim(&mut self) {
        self.0.fullscreen.trim();
    }
}

impl shader::Pipeline for VeilGpu {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self(Gpu::new(device, format, "fs_veil"))
    }

    fn trim(&mut self) {
        self.0.fullscreen.trim();
    }
}

impl ShaderPrimitive for WashPass {
    type Pipeline = WashGpu;

    fn prepare(
        &self,
        pipeline: &mut WashGpu,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &shader::Viewport,
    ) {
        pipeline.0.write(device, queue, &self.1, self.0);
    }

    fn draw(&self, pipeline: &WashGpu, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pipeline.0.fullscreen.draw(pass, &self.1);
        true
    }
}

impl ShaderPrimitive for VeilPass {
    type Pipeline = VeilGpu;

    fn prepare(
        &self,
        pipeline: &mut VeilGpu,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &shader::Viewport,
    ) {
        pipeline.0.write(device, queue, &self.1, self.0);
    }

    fn draw(&self, pipeline: &VeilGpu, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pipeline.0.fullscreen.draw(pass, &self.1);
        true
    }
}
