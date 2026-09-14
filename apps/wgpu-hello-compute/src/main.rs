use std::{num::NonZeroU64, str::FromStr};
use wgpu::util::DeviceExt;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments: Vec<f32> = std::env::args()
        .skip(1)
        .map(|s| f32::from_str(&s))
        .collect::<Result<_, _>>()?;

    if arguments.is_empty() {
        arguments = vec![1.0, 2.0, 3.0, 4.0];
    }

    println!("Parsed {} arguments", arguments.len());

    println!("Input: {arguments:?}");
    if wgpu::Instance::enabled_backend_features().is_empty() {
        return Err("No wgpu GPU backend is implemented for this target. The TRUEOS Blueprint is packaged, but compute requires a TRUEOS wgpu backend.".into());
    }

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

    #[cfg(target_os = "trueos")]
    {
        // Only borrow the HAL instance to report its completed native probe.
        let hal = unsafe { instance.as_hal::<wgpu::hal::api::TrueOs>() }
            .ok_or("TRUEOS HAL initialization failed: native vGPU probe unavailable")?;
        println!("TRUEOS vGPU probe: {:?}", hal.probe_info());
    }

    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .map_err(|error| format!("No usable compute adapter: {error}. The TRUEOS HAL currently probes vGPU, but its shader/dispatch/readback path is not implemented."))?;

    println!("Running on Adapter: {:#?}", adapter.get_info());

    let downlevel_capabilities = adapter.get_downlevel_capabilities();
    if !downlevel_capabilities
        .flags
        .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
    {
        panic!("Adapter does not support compute shaders");
    }

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: None,
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults(),
        default_queue: wgpu::QueueDescriptor { label: None },
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::Off,
    }))
    .expect("Failed to create device");

    let module = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

    let input_data_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&arguments),
        usage: wgpu::BufferUsages::STORAGE,
    });

    let output_data_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: input_data_buffer.size(),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    let download_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: input_data_buffer.size(),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    min_binding_size: Some(NonZeroU64::new(4).unwrap()),
                    has_dynamic_offset: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    min_binding_size: Some(NonZeroU64::new(4).unwrap()),
                    has_dynamic_offset: false,
                },
                count: None,
            },
        ],
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: input_data_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output_data_buffer.as_entire_binding(),
            },
        ],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });

    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("doubleMe"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

    let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: None,
        timestamp_writes: None,
    });

    compute_pass.set_pipeline(&pipeline);
    compute_pass.set_bind_group(0, &bind_group, &[]);

    let workgroup_count = arguments.len().div_ceil(64);
    compute_pass.dispatch_workgroups(workgroup_count as u32, 1, 1);

    drop(compute_pass);

    encoder.copy_buffer_to_buffer(
        &output_data_buffer,
        0,
        &download_buffer,
        0,
        output_data_buffer.size(),
    );

    let command_buffer = encoder.finish();

    queue.submit([command_buffer]);

    let buffer_slice = download_buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });

    device.poll(wgpu::PollType::wait_indefinitely())?;
    receiver.recv()??;

    let data = buffer_slice.get_mapped_range().unwrap();
    let result: Vec<f32> = bytemuck::allocation::pod_collect_to_vec(&data);

    println!("Result: {result:?}");
    if !result.iter().zip(&arguments).all(|(actual, input)| {
        let expected = input * 2.0;
        *actual == expected || (actual.is_nan() && expected.is_nan())
    }) || result.len() != arguments.len()
    {
        return Err("GPU result did not match the expected doubled inputs".into());
    }
    drop(data);
    download_buffer.unmap();
    println!("PASS: GPU output matches doubled inputs");
    Ok(())
}
