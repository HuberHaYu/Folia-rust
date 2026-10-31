// Full-window vignette, grain, and the subtitle radial glow.

mod frame;
mod pipeline;
mod wash;

pub use frame::grain_frame;
pub use wash::{Glow, Veil, Wash};

use pipeline::Fullscreen;

use std::marker::PhantomData;

use iced::widget::shader::{self, Primitive as ShaderPrimitive};
use iced::{Rectangle, mouse};

const SHADER: &str = concat!(include_str!("../shaders/fullscreen.wgsl"), include_str!("../shaders/atmosphere.wgsl"));

/// What one frame of the atmosphere pass draws. Coordinates are fractions of the widget.
#[derive(Debug, Clone, Copy)]
pub struct Atmosphere {
    pub time: f32,
    pub vignette: f32,
    pub grain: f32,
    /// Center of the subtitle ellipse, as fractions of the widget. Y grows downward.
    pub glow_center: [f32; 2],
    /// Semi-axes of the 115% × 130% ellipse, as fractions of the widget.
    pub glow_radius: [f32; 2],
    /// Theme background. The glow is this color, not the accent.
    pub glow_rgb: [f32; 3],
    /// Parent opacity of the subtitle block. Zero hides the glow.
    pub glow_alpha: f32,
}

/// iced shader program. It has no widget state of its own.
#[derive(Debug, Clone, Copy)]
pub struct Program<Message> {
    pub atmosphere: Atmosphere,
    _message: PhantomData<Message>,
}

impl<Message> Program<Message> {
    pub fn new(atmosphere: Atmosphere) -> Self {
        Self { atmosphere, _message: PhantomData }
    }
}

impl<Message> shader::Program<Message> for Program<Message> {
    type State = ();
    type Primitive = Pass;

    fn draw(&self, _state: &Self::State, _cursor: mouse::Cursor, bounds: Rectangle) -> Self::Primitive {
        Pass { atmosphere: self.atmosphere, size: [bounds.width.max(1.0), bounds.height.max(1.0)] }
    }
}

#[derive(Debug)]
pub struct Pass {
    atmosphere: Atmosphere,
    size: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    resolution: [f32; 2],
    time: f32,
    vignette: f32,
    glow_center: [f32; 2],
    glow_radius: [f32; 2],
    glow_rgb: [f32; 3],
    glow_alpha: f32,
    grain: f32,
    frame: f32,
    _pad0: f32,
    _pad1: f32,
}

pub struct Gpu(Fullscreen);

impl ShaderPrimitive for Pass {
    type Pipeline = Gpu;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &iced::widget::shader::Viewport,
    ) {
        let atmosphere = &self.atmosphere;
        let uniforms = Uniforms {
            resolution: self.size,
            time: atmosphere.time,
            vignette: atmosphere.vignette,
            glow_center: atmosphere.glow_center,
            glow_radius: atmosphere.glow_radius,
            glow_rgb: atmosphere.glow_rgb,
            glow_alpha: atmosphere.glow_alpha,
            grain: atmosphere.grain,
            frame: grain_frame(atmosphere.time) as f32,
            _pad0: 0.0,
            _pad1: 0.0,
        };
        queue.write_buffer(&pipeline.0.uniform, 0, bytemuck::bytes_of(&uniforms));
    }

    fn draw(&self, pipeline: &Self::Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pipeline.0.draw(pass);
        true
    }
}

impl shader::Pipeline for Gpu {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self(Fullscreen::new(device, format, "folia-atmosphere", SHADER, "fs_overlay", std::mem::size_of::<Uniforms>()))
    }
}
