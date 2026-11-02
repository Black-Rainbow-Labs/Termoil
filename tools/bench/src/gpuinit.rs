//! Where GPU start up time goes: instance, adapter, device and vello's pipelines, per wgpu backend.
//! Used to pick the backend and the start up order behind the P4 gate.

use std::time::Instant;

/// Times each start up step for one backend and prints a line.
fn time_backend(name: &str, backends: wgpu::Backends) {
    let t0 = Instant::now();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let t_instance = t0.elapsed();
    let opts = wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: false,
    };
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&opts)) else {
        println!("{name:<8} no adapter");
        return;
    };
    let t_adapter = t0.elapsed();
    let desc = wgpu::DeviceDescriptor { label: Some("probe"), ..Default::default() };
    let Ok((device, _queue)) = pollster::block_on(adapter.request_device(&desc)) else {
        println!("{name:<8} no device");
        return;
    };
    let t_device = t0.elapsed();
    let config = vello_gpu::RenderTargetConfig { format: wgpu::TextureFormat::Rgba8Unorm, width: 800, height: 600 };
    let _r = vello_gpu::Renderer::new(&device, &config);
    let t_renderer = t0.elapsed();
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
    println!(
        "{name:<8} instance {:7.1} ms  adapter {:7.1} ms  device {:7.1} ms  vello pipelines {:7.1} ms  total {:7.1} ms",
        ms(t_instance),
        ms(t_adapter - t_instance),
        ms(t_device - t_adapter),
        ms(t_renderer - t_device),
        ms(t_renderer)
    );
}

/// Runs the timing for the backends named on the command line (all, vulkan, dx12, gl).
pub fn run(which: &str) {
    let all = [
        ("vulkan", wgpu::Backends::VULKAN),
        ("dx12", wgpu::Backends::DX12),
        ("gl", wgpu::Backends::GL),
        ("all", wgpu::Backends::all()),
    ];
    for (name, b) in all {
        if which == "every" || which == name {
            time_backend(name, b);
        }
    }
}
