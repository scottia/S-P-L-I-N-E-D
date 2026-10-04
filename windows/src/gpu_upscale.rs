//! Optional hardware-accelerated conventional artwork enlargement.
//!
//! This is deliberately not an AI model. It runs a Lanczos-3 resampler on a
//! hardware Vulkan adapter and returns ordinary RGBA pixels. Callers retain a
//! deterministic CPU Lanczos fallback when initialization or execution fails.

use image::RgbaImage;
use std::sync::OnceLock;
use wgpu::util::DeviceExt;

const WORKGROUP_SIDE: u32 = 8;

const LANCZOS_SHADER: &str = r#"
struct Pixels {
    values: array<u32>,
};

struct Dimensions {
    source_width: u32,
    source_height: u32,
    target_width: u32,
    target_height: u32,
};

@group(0) @binding(0) var<storage, read> source: Pixels;
@group(0) @binding(1) var<storage, read_write> destination: Pixels;
@group(0) @binding(2) var<uniform> dimensions: Dimensions;

fn unpack_rgba(pixel: u32) -> vec4<f32> {
    return vec4<f32>(
        f32(pixel & 255u),
        f32((pixel >> 8u) & 255u),
        f32((pixel >> 16u) & 255u),
        f32((pixel >> 24u) & 255u)
    ) / 255.0;
}

fn pack_rgba(value: vec4<f32>) -> u32 {
    let rounded = vec4<u32>(clamp(value, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0 + 0.5);
    return rounded.x | (rounded.y << 8u) | (rounded.z << 16u) | (rounded.w << 24u);
}

fn sinc(value: f32) -> f32 {
    if abs(value) < 0.000001 {
        return 1.0;
    }
    let angle = 3.141592653589793 * value;
    return sin(angle) / angle;
}

fn lanczos3(value: f32) -> f32 {
    let distance = abs(value);
    if distance >= 3.0 {
        return 0.0;
    }
    return sinc(value) * sinc(value / 3.0);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= dimensions.target_width || id.y >= dimensions.target_height {
        return;
    }

    let source_x = (f32(id.x) + 0.5) * f32(dimensions.source_width) /
        f32(dimensions.target_width) - 0.5;
    let source_y = (f32(id.y) + 0.5) * f32(dimensions.source_height) /
        f32(dimensions.target_height) - 0.5;
    let base_x = i32(floor(source_x));
    let base_y = i32(floor(source_y));
    var accumulated = vec4<f32>(0.0);
    var weight_sum = 0.0;

    for (var offset_y = -2; offset_y <= 3; offset_y = offset_y + 1) {
        let sample_y = clamp(base_y + offset_y, 0, i32(dimensions.source_height) - 1);
        let weight_y = lanczos3(source_y - f32(base_y + offset_y));
        for (var offset_x = -2; offset_x <= 3; offset_x = offset_x + 1) {
            let sample_x = clamp(base_x + offset_x, 0, i32(dimensions.source_width) - 1);
            let weight = lanczos3(source_x - f32(base_x + offset_x)) * weight_y;
            let source_index = u32(sample_y) * dimensions.source_width + u32(sample_x);
            accumulated = accumulated + unpack_rgba(source.values[source_index]) * weight;
            weight_sum = weight_sum + weight;
        }
    }

    if abs(weight_sum) > 0.000001 {
        accumulated = accumulated / weight_sum;
    }
    let target_index = id.y * dimensions.target_width + id.x;
    destination.values[target_index] = pack_rgba(accumulated);
}
"#;

struct GpuUpscaler {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    adapter_name: String,
    max_storage_buffer_size: u64,
}

static GPU: OnceLock<Result<GpuUpscaler, String>> = OnceLock::new();

pub(crate) fn resize_lanczos3(
    source: &RgbaImage,
    target_width: u32,
    target_height: u32,
) -> Result<(RgbaImage, &'static str), String> {
    if source.width() == 0 || source.height() == 0 || target_width == 0 || target_height == 0 {
        return Err("GPU upscale dimensions must be greater than zero.".to_string());
    }
    let upscaler = GPU
        .get_or_init(GpuUpscaler::new)
        .as_ref()
        .map_err(Clone::clone)?;
    upscaler
        .resize(source, target_width, target_height)
        .map(|image| (image, upscaler.adapter_name.as_str()))
}

impl GpuUpscaler {
    fn new() -> Result<Self, String> {
        let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_descriptor.backends = wgpu::Backends::VULKAN;
        let instance = wgpu::Instance::new(instance_descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .map_err(|error| format!("no compatible hardware Vulkan adapter: {error}"))?;
        let info = adapter.get_info();
        if matches!(info.device_type, wgpu::DeviceType::Cpu) {
            return Err("the selected Vulkan adapter is software-only".to_string());
        }
        let limits = adapter.limits();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("SPLINED GPU upscaler"),
            required_features: wgpu::Features::empty(),
            required_limits: limits.clone(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|error| format!("unable to create GPU upscaler device: {error}"))?;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("SPLINED Lanczos-3 upscaler"),
            source: wgpu::ShaderSource::Wgsl(LANCZOS_SHADER.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("SPLINED Lanczos-3 pipeline"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let bind_group_layout = pipeline.get_bind_group_layout(0);
        Ok(Self {
            device,
            queue,
            pipeline,
            bind_group_layout,
            adapter_name: info.name,
            max_storage_buffer_size: limits.max_storage_buffer_binding_size,
        })
    }

    fn resize(
        &self,
        source: &RgbaImage,
        target_width: u32,
        target_height: u32,
    ) -> Result<RgbaImage, String> {
        let source_size = byte_len(source.width(), source.height())?;
        let target_size = byte_len(target_width, target_height)?;
        if source_size > self.max_storage_buffer_size || target_size > self.max_storage_buffer_size
        {
            return Err(format!(
                "image requires source/target buffers of {source_size}/{target_size} bytes, exceeding the GPU storage-buffer limit of {} bytes",
                self.max_storage_buffer_size
            ));
        }

        let source_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("SPLINED upscale source"),
                contents: source.as_raw(),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let target_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("SPLINED upscale target"),
            size: target_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("SPLINED upscale readback"),
            size: target_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let dimensions = [source.width(), source.height(), target_width, target_height];
        let dimension_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("SPLINED upscale dimensions"),
                contents: bytemuck::cast_slice(&dimensions),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("SPLINED upscale bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: source_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: target_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: dimension_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("SPLINED upscale commands"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("SPLINED Lanczos-3 pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(
                target_width.div_ceil(WORKGROUP_SIDE),
                target_height.div_ceil(WORKGROUP_SIDE),
                1,
            );
        }
        encoder.copy_buffer_to_buffer(&target_buffer, 0, &readback, 0, target_size);
        self.queue.submit(Some(encoder.finish()));

        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| format!("GPU upscale wait failed: {error}"))?;
        receiver
            .recv()
            .map_err(|error| format!("GPU upscale readback callback failed: {error}"))?
            .map_err(|error| format!("GPU upscale readback failed: {error}"))?;

        let mapped = readback.slice(..).get_mapped_range();
        let bytes = mapped.to_vec();
        drop(mapped);
        readback.unmap();
        RgbaImage::from_raw(target_width, target_height, bytes)
            .ok_or_else(|| "GPU upscaler returned an invalid RGBA buffer.".to_string())
    }
}

fn byte_len(width: u32, height: u32) -> Result<u64, String> {
    u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "GPU upscale image dimensions overflowed the buffer size.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_length_is_checked_rgba() {
        assert_eq!(byte_len(1800, 1800).unwrap(), 12_960_000);
        assert_eq!(byte_len(0, 1800).unwrap(), 0);
    }

    #[test]
    fn hardware_lanczos_preserves_a_solid_rgba_image_when_available() {
        let source = RgbaImage::from_pixel(4, 4, image::Rgba([24, 96, 180, 255]));
        let Ok((resized, adapter)) = resize_lanczos3(&source, 16, 16) else {
            // Hardware acceleration is optional; production takes the tested
            // CPU Lanczos fallback on machines without a Vulkan adapter.
            return;
        };
        assert!(!adapter.trim().is_empty());
        eprintln!("SPLINED GPU test adapter: {adapter}");
        assert_eq!(resized.dimensions(), (16, 16));
        for pixel in resized.pixels() {
            assert_eq!(*pixel, image::Rgba([24, 96, 180, 255]));
        }
    }

    #[test]
    fn hardware_lanczos_tracks_cpu_lanczos_on_a_gradient_when_available() {
        let source = RgbaImage::from_fn(9, 7, |x, y| {
            image::Rgba([
                u8::try_from(x * 23).unwrap(),
                u8::try_from(y * 31).unwrap(),
                u8::try_from((x + y) * 13).unwrap(),
                255,
            ])
        });
        let Ok((gpu, _)) = resize_lanczos3(&source, 27, 21) else {
            return;
        };
        let cpu = image::imageops::resize(&source, 27, 21, image::imageops::FilterType::Lanczos3);
        let maximum_channel_delta = gpu
            .as_raw()
            .iter()
            .zip(cpu.as_raw())
            .map(|(left, right)| left.abs_diff(*right))
            .max()
            .unwrap_or(0);
        assert!(
            maximum_channel_delta <= 3,
            "GPU and CPU Lanczos differed by {maximum_channel_delta} levels"
        );
    }
}
