// One full-screen triangle drawn by a WGSL fragment shader, with a uniform buffer per primitive in the frame.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Pipeline and uniform buffers shared by every full-screen pass of one kind.
///
/// iced prepares every primitive of a frame before drawing any, so each primitive writes its own
/// buffer. Buffers are kept across frames and only grow with the most primitives seen in one frame.
pub struct Fullscreen {
    pub pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    label: String,
    uniform_size: u64,
    slots: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
    used: usize,
}

/// The uniform buffer a primitive was prepared into, read back when it draws.
#[derive(Debug, Default)]
pub struct Slot(AtomicUsize);

impl Fullscreen {
    /// `source` must define `vs`, `fragment` and a uniform at group 0, binding 0 of `uniform_size` bytes.
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        label: &str,
        source: &str,
        fragment: &str,
        uniform_size: usize,
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(fragment),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self {
            pipeline,
            layout,
            label: label.to_owned(),
            uniform_size: uniform_size as u64,
            slots: Vec::new(),
            used: 0,
        }
    }

    /// Uploads one primitive's uniforms into the next free buffer and records it in `slot`.
    pub fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, slot: &Slot, bytes: &[u8]) {
        if self.used == self.slots.len() {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&self.label),
                size: self.uniform_size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&self.label),
                layout: &self.layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
            });
            self.slots.push((buffer, bind));
        }
        if let Some((buffer, _)) = self.slots.get(self.used) {
            queue.write_buffer(buffer, 0, bytes);
        }
        slot.0.store(self.used, Ordering::Relaxed);
        self.used += 1;
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, slot: &Slot) {
        let Some((_, bind)) = self.slots.get(slot.0.load(Ordering::Relaxed)) else { return };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Frees every buffer for the next frame's primitives.
    pub fn trim(&mut self) {
        self.used = 0;
    }
}
