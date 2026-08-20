use bytemuck::{Pod, Zeroable};
use image::RgbaImage;
use std::sync::OnceLock;
use wgpu::util::DeviceExt;

const BLUR_SHADER: &str = r#"
struct Params {
    width: u32,
    height: u32,
    radius: u32,
    horizontal: u32,
}

@group(0) @binding(0) var<storage, read> source: array<u32>;
@group(0) @binding(1) var<storage, read_write> destination: array<u32>;
@group(0) @binding(2) var<uniform> params: Params;

fn channel(pixel: u32, shift: u32) -> u32 {
    return (pixel >> shift) & 255u;
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = id.x;
    let y = id.y;
    if (x >= params.width || y >= params.height) { return; }
    let index = y * params.width + x;
    var sums = vec4<u32>(0u);
    let r = i32(params.radius);
    for (var offset = -r; offset <= r; offset = offset + 1) {
        var sx = i32(x);
        var sy = i32(y);
        if (params.horizontal != 0u) {
            sx = clamp(sx + offset, 0, i32(params.width) - 1);
        } else {
            sy = clamp(sy + offset, 0, i32(params.height) - 1);
        }
        let pixel = source[u32(sy) * params.width + u32(sx)];
        sums = sums + vec4<u32>(
            channel(pixel, 0u), channel(pixel, 8u),
            channel(pixel, 16u), channel(pixel, 24u));
    }
    let divisor = params.radius * 2u + 1u;
    let rounded = (sums + vec4<u32>(divisor / 2u)) / vec4<u32>(divisor);
    destination[index] = rounded.x | (rounded.y << 8u) |
        (rounded.z << 16u) | (rounded.w << 24u);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    width: u32,
    height: u32,
    radius: u32,
    horizontal: u32,
}

struct GpuBlur {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    adapter_name: String,
}

impl GpuBlur {
    fn new() -> Result<Self, String> {
        pollster::block_on(async {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends: wgpu::Backends::DX12,
                ..Default::default()
            });
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
                .map_err(|error| format!("GPU adapter unavailable: {error}"))?;
            let adapter_name = adapter.get_info().name;
            let adapter_limits = adapter.limits();
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("LiteExif GPU device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: adapter_limits,
                    memory_hints: wgpu::MemoryHints::Performance,
                    trace: wgpu::Trace::Off,
                })
                .await
                .map_err(|error| format!("GPU device unavailable: {error}"))?;
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("LiteExif integer box blur"),
                source: wgpu::ShaderSource::Wgsl(BLUR_SHADER.into()),
            });
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("LiteExif blur bindings"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("LiteExif blur pipeline layout"),
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("LiteExif blur pipeline"),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
            Ok(Self {
                device,
                queue,
                pipeline,
                layout,
                adapter_name,
            })
        })
    }

    fn blur(&self, image: &RgbaImage, radius: u32) -> Result<RgbaImage, String> {
        let byte_len = image.as_raw().len() as u64;
        let buffer_a = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("LiteExif blur input"),
                contents: image.as_raw(),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });
        let buffer_b = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("LiteExif blur scratch"),
            size: byte_len,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("LiteExif blur readback"),
            size: byte_len,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for pass_index in 0..6u32 {
            let (source, destination) = if pass_index % 2 == 0 {
                (&buffer_a, &buffer_b)
            } else {
                (&buffer_b, &buffer_a)
            };
            let params = Params {
                width: image.width(),
                height: image.height(),
                radius,
                horizontal: u32::from(pass_index % 2 == 0),
            };
            let uniform = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("LiteExif blur params"),
                    contents: bytemuck::bytes_of(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("LiteExif blur group"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: source.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: destination.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform.as_entire_binding(),
                    },
                ],
            });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("LiteExif box blur pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(image.width().div_ceil(16), image.height().div_ceil(16), 1);
        }
        encoder.copy_buffer_to_buffer(&buffer_a, 0, &staging, 0, byte_len);
        let submission = self.queue.submit(Some(encoder.finish()));
        let slice = staging.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::WaitForSubmissionIndex(submission))
            .map_err(|error| format!("GPU polling failed: {error:?}"))?;
        receiver
            .recv()
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        let mapped = slice.get_mapped_range();
        let bytes = mapped.to_vec();
        drop(mapped);
        staging.unmap();
        RgbaImage::from_raw(image.width(), image.height(), bytes)
            .ok_or_else(|| "GPU returned an invalid image buffer".to_owned())
    }
}

static GPU: OnceLock<Result<GpuBlur, String>> = OnceLock::new();

pub fn blur(image: &RgbaImage, radius: u32) -> Result<(RgbaImage, &str), String> {
    let gpu = GPU
        .get_or_init(GpuBlur::new)
        .as_ref()
        .map_err(Clone::clone)?;
    let output = gpu.blur(image, radius)?;
    Ok((output, &gpu.adapter_name))
}

pub fn adapter_name() -> Option<&'static str> {
    GPU.get()
        .and_then(|gpu| gpu.as_ref().ok())
        .map(|gpu| gpu.adapter_name.as_str())
}
