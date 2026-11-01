// Rounded fill with a CSS box-shadow and a 1px ring. Each plate in a frame keeps its own uniforms.

use iced::widget::shader::{self, Primitive as ShaderPrimitive};
use iced::{Rectangle, mouse};

use crate::pipeline::{Fullscreen, Slot};

const SHADER: &str = concat!(include_str!("../shaders/fullscreen.wgsl"), include_str!("../shaders/plate.wgsl"));

/// A rounded rectangle of `fill` at `fill_origin` / `fill_size` with `0 shadow_y shadow_blur shadow_spread shadow`
/// and `0 0 0 1px ring` around it. A non-positive `fill_size.y` uses the widget height. The widget must be
/// large enough to hold the shadow; it fades out over the last 1.5px of the widget. Backdrop blur is not drawn.
#[derive(Debug, Clone, Copy)]
pub struct Plate {
    pub radius: f32,
    pub fill_origin: [f32; 2],
    pub fill_size: [f32; 2],
    pub fill: [f32; 4],
    pub shadow: [f32; 4],
    pub shadow_blur: f32,
    pub shadow_y: f32,
    pub shadow_spread: f32,
    pub ring: [f32; 4],
}

impl<Message> shader::Program<Message> for Plate {
    type State = ();
    type Primitive = PlatePass;

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        mouse::Interaction::None
    }

    fn draw(&self, _state: &Self::State, _cursor: mouse::Cursor, bounds: Rectangle) -> Self::Primitive {
        let resolution = [bounds.width.max(1.0), bounds.height.max(1.0)];
        let mut size = self.fill_size;
        if size[1] <= 0.0 {
            size[1] = resolution[1];
        }
        let uniforms = Uniforms {
            resolution,
            encode: 0.0,
            radius: self.radius,
            origin: self.fill_origin,
            size,
            fill: self.fill,
            shadow: self.shadow,
            shadow_blur: self.shadow_blur,
            shadow_y: self.shadow_y,
            shadow_spread: self.shadow_spread,
            _pad: 0.0,
            ring: self.ring,
        };
        PlatePass(uniforms, Slot::default())
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    resolution: [f32; 2],
    encode: f32,
    radius: f32,
    origin: [f32; 2],
    size: [f32; 2],
    fill: [f32; 4],
    shadow: [f32; 4],
    shadow_blur: f32,
    shadow_y: f32,
    shadow_spread: f32,
    _pad: f32,
    ring: [f32; 4],
}

#[derive(Debug)]
pub struct PlatePass(Uniforms, Slot);

pub struct PlateGpu {
    fullscreen: Fullscreen,
    srgb_target: bool,
}

impl shader::Pipeline for PlateGpu {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self {
            fullscreen: Fullscreen::new(device, format, "fs_plate", SHADER, "fs_plate", size_of::<Uniforms>()),
            srgb_target: format.is_srgb(),
        }
    }

    fn trim(&mut self) {
        self.fullscreen.trim();
    }
}

impl ShaderPrimitive for PlatePass {
    type Pipeline = PlateGpu;

    fn prepare(
        &self,
        pipeline: &mut PlateGpu,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &shader::Viewport,
    ) {
        let uniforms = Uniforms { encode: if pipeline.srgb_target { 1.0 } else { 0.0 }, ..self.0 };
        pipeline.fullscreen.write(device, queue, &self.1, bytemuck::bytes_of(&uniforms));
    }

    fn draw(&self, pipeline: &PlateGpu, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pipeline.fullscreen.draw(pass, &self.1);
        true
    }
}
