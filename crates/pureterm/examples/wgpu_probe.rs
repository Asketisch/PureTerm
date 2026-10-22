//! 启动耗时探针:测量 wgpu 各阶段在 macOS 上的真实成本。
//!
//! 用法:`cargo run --release --example wgpu_probe`
//!
//! 输出每一步的毫秒数,用于判断「窗口迟迟不出」到底卡在
//! 适配器枚举、设备创建还是表面配置。

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Instant;

/// 极简 `block_on` —— 避免为这个一次性探针引入 `futures` 依赖。
fn block_on<F: Future>(mut fut: F) -> F::Output {
    struct NoopWaker;
    impl Wake for NoopWaker {
        fn wake(self: Arc<Self>) {}
    }
    let waker = Waker::from(Arc::new(NoopWaker));
    let mut cx = Context::from_waker(&waker);
    let mut fut = pin!(fut);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

fn main() {
    let t0 = Instant::now();
    println!("process_start        {:.1} ms", t0.elapsed().as_secs_f64() * 1000.0);

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
        flags: wgpu::InstanceFlags::empty(),
        ..Default::default()
    });
    println!("instance_created     {:.1} ms", t0.elapsed().as_secs_f64() * 1000.0);

    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("no adapter");
    println!("adapter_ready        {:.1} ms", t0.elapsed().as_secs_f64() * 1000.0);

    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("probe"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits {
            max_bind_groups: 2,
            ..wgpu::Limits::default()
        },
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))
    .expect("no device");
    println!("device_ready         {:.1} ms", t0.elapsed().as_secs_f64() * 1000.0);

    // 模拟终端管线的 4 个 WGSL shader + 4 条渲染管线。
    let src = include_str!("../../pureterm-alacritty-runtime/src/alacritty/renderer/shaders/bg_color.wgsl");
    let t_shader = Instant::now();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("probe bg"),
        source: wgpu::ShaderSource::Wgsl(src.into()),
    });
    println!("one_wgsl_module      {:.1} ms (total {:.1})", t_shader.elapsed().as_secs_f64() * 1000.0, t0.elapsed().as_secs_f64() * 1000.0);

    let t_pipeline = Instant::now();
    let _pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("probe pipeline"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });
    println!("one_pipeline         {:.1} ms (total {:.1})", t_pipeline.elapsed().as_secs_f64() * 1000.0, t0.elapsed().as_secs_f64() * 1000.0);

    // 再建一个 shader,看 Metal 驱动是否缓存了编译结果。
    let t_second = Instant::now();
    let _module2 = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("probe bg 2"),
        source: wgpu::ShaderSource::Wgsl(src.into()),
    });
    println!("second_wgsl_module   {:.1} ms (total {:.1})", t_second.elapsed().as_secs_f64() * 1000.0, t0.elapsed().as_secs_f64() * 1000.0);

    drop(queue);
    println!("done                 {:.1} ms", t0.elapsed().as_secs_f64() * 1000.0);
}
