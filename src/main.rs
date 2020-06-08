extern crate glfw;

use glfw::{Action, Context, Key};
use futures::executor::block_on;

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 3],
}

unsafe impl bytemuck::Pod for Vertex {}
unsafe impl bytemuck::Zeroable for Vertex {}

impl Vertex {
    fn desc<'a>() -> wgpu::VertexBufferDescriptor<'a> {
        use std::mem;
        wgpu::VertexBufferDescriptor {
            stride: mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::InputStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttributeDescriptor {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float3,
                },
                wgpu::VertexAttributeDescriptor {
                    offset: mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float3,
                },
            ]
        }
    }
}


const VERTICES: &[Vertex] = &[
    Vertex { position: [0.0, 0.5, 0.0], color: [1.0, 0.0, 0.0] },
    Vertex { position: [-0.5, -0.5, 0.0], color: [0.0, 1.0, 0.0] },
    Vertex { position: [0.5, -0.5, 0.0], color: [0.0, 0.0, 1.0] },
];

const INDICES: &[u16] = &[
    0, 1, 2,
];

fn main() {
    let mut glfw = glfw::init(glfw::FAIL_ON_ERRORS).unwrap();
    let (mut window, events) = glfw.create_window(1280, 720, "Hello this is window", glfw::WindowMode::Windowed)
        .expect("Failed to create GLFW window.");

    window.set_all_polling(true);
    window.make_current();

    let mut state = block_on(WgpuState::new(&window));

    while !window.should_close() {
        glfw.poll_events();
        for (_, event) in glfw::flush_messages(&events) {
            state.input(&mut window, &event);
        }
        // events handled, now draw B)
        state.update();
        state.render(&mut window);
    }
}

struct ImguiState {
    context: imgui::Context,
    renderer: imgui_wgpu::Renderer,
    platform: imgui_glfw_support::GlfwPlatform,
    last_cursor: Option<imgui::MouseCursor>,
    should_render: bool,
}

// main.rs
struct WgpuState {
    surface: wgpu::Surface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    sc_desc: wgpu::SwapChainDescriptor,
    swap_chain: wgpu::SwapChain,

    render_pipeline: wgpu::RenderPipeline,

    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    num_indices: u32,
    clear_color: wgpu::Color,

    size: (i32, i32),

    imgui: ImguiState,
}

impl WgpuState {
    async fn new(window: &glfw::Window) -> Self {
        let size = window.get_framebuffer_size(); // TODO: eh?
        let clear_color = wgpu::Color { r: 0.1, g: 0.2, b: 0.3, a: 0.2};
        let format = wgpu::TextureFormat::Bgra8UnormSrgb;

        let surface = wgpu::Surface::create(window);

        let adapter = {
            wgpu::Adapter::request(
                &wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::Default,
                    compatible_surface: Some(&surface),
                },
                wgpu::BackendBit::PRIMARY, // vulkan/metal/dx12/wgpu
            ).await.unwrap()
        };

        let (device, mut queue) = {
            adapter.request_device(&wgpu::DeviceDescriptor {
                extensions: wgpu::Extensions {
                    anisotropic_filtering: false, //????????
                },
                limits: Default::default(),
            }).await
        };

        let sc_desc = wgpu::SwapChainDescriptor {
            usage: wgpu::TextureUsage::OUTPUT_ATTACHMENT,
            format,
            width: size.0 as u32,
            height: size.1 as u32,
            present_mode: wgpu::PresentMode::Fifo, // TODO: FIFO
        };

        let swap_chain = device.create_swap_chain(&surface, &sc_desc);

        let imgui = {
            let mut imgui = imgui::Context::create();
            imgui.set_ini_filename(None);
            let mut glfw_platform = imgui_glfw_support::GlfwPlatform::init(&mut imgui);

            glfw_platform.attach_window(
                imgui.io_mut(),
                &window,
                imgui_glfw_support::HiDpiMode::Default,
            );

            unsafe {
                glfw_platform.set_clipboard_backend(&mut imgui, &window);
            }

            let imgui_renderer = imgui_wgpu::Renderer::new(
                &mut imgui,
                &device,
                &mut queue,
                sc_desc.format,
                None,
            );

            ImguiState {
                context: imgui,
                platform: glfw_platform,
                renderer: imgui_renderer,
                last_cursor: None,
                should_render: false,
            }
        };

        queue.submit(&[]);

        // # set up a render pipeline
        // ## compile shaders
        let (vs_module, fs_module) = {
            let vs_src = include_str!("shader.vert");
            let fs_src = include_str!("shader.frag");

            let mut compiler = shaderc::Compiler::new().unwrap();
            let vs_spirv = compiler.compile_into_spirv(vs_src, shaderc::ShaderKind::Vertex, "shader.vert", "main", None).unwrap();
            let fs_spirv = compiler.compile_into_spirv(fs_src, shaderc::ShaderKind::Fragment, "shader.frag", "main", None).unwrap();

            let vs_data = wgpu::read_spirv(std::io::Cursor::new(vs_spirv.as_binary_u8())).unwrap();
            let fs_data = wgpu::read_spirv(std::io::Cursor::new(fs_spirv.as_binary_u8())).unwrap();

            let vs_module = device.create_shader_module(&vs_data);
            let fs_module = device.create_shader_module(&fs_data);
            (vs_module, fs_module)
        };

        // ## create pipeline
        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            bind_group_layouts: &[],
        });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            layout: &render_pipeline_layout,
            vertex_stage: wgpu::ProgrammableStageDescriptor {
                module: &vs_module,
                entry_point: "main",
            },
            fragment_stage: Some(wgpu::ProgrammableStageDescriptor {
                module: &fs_module,
                entry_point: "main",
            }),
            rasterization_state: Some(wgpu::RasterizationStateDescriptor {
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: wgpu::CullMode::Back,
                depth_bias: 0,
                depth_bias_slope_scale: 0.0,
                depth_bias_clamp: 0.0,
            }),
            color_states: &[
                // swapchain B)
                wgpu::ColorStateDescriptor {
                    format: sc_desc.format,
                    color_blend: wgpu::BlendDescriptor::REPLACE,
                    alpha_blend: wgpu::BlendDescriptor::REPLACE,
                    write_mask:wgpu::ColorWrite::ALL,
                },
            ],
            primitive_topology: wgpu::PrimitiveTopology::TriangleList,
            depth_stencil_state: None,
            vertex_state: wgpu::VertexStateDescriptor {
                index_format: wgpu::IndexFormat::Uint16,
                vertex_buffers: &[Vertex::desc()],
            },
            sample_count: 1,
            sample_mask: !0,
            alpha_to_coverage_enabled: true,
        });

        // ## create buffers
        let vertex_buffer = {
            device.create_buffer_with_data(
                bytemuck::cast_slice(VERTICES),
                wgpu::BufferUsage::VERTEX
            )
        };
        let index_buffer = {
            device.create_buffer_with_data(
                bytemuck::cast_slice(INDICES),
                wgpu::BufferUsage::INDEX
            )
        };
        let num_indices = INDICES.len() as u32;

        Self {
            surface,
            device,
            queue,
            sc_desc,
            swap_chain,
            render_pipeline,
            vertex_buffer,
            index_buffer,
            num_indices,
            clear_color,
            size,
            imgui,
        }
    }

    fn resize(&mut self, new_size: (i32, i32)) {
        self.size = new_size;
        self.sc_desc.width = new_size.0 as u32;
        self.sc_desc.height = new_size.1 as u32;
        self.swap_chain = self.device.create_swap_chain(&self.surface, &self.sc_desc);
        println!("resizing to: {}, {}", new_size.0, new_size.1);
    }

    // input() won't deal with GPU code, so it can be synchronous
    fn input(&mut self, window: &mut glfw::Window, event: &glfw::WindowEvent) -> bool {
        if self.imgui.should_render {
            self.imgui.platform.handle_event(self.imgui.context.io_mut(), &window, &event);
        }
        match event {
            glfw::WindowEvent::Key(Key::Escape, _, Action::Press, _) => {
                window.set_should_close(true)
            },
            glfw::WindowEvent::Key(Key::F3, _, Action::Press, _) => {
                self.imgui.should_render = !self.imgui.should_render;
            },
            glfw::WindowEvent::Size(w, h) => {
                self.resize((*w, *h));
            },
            _ => {}
        }
        false
    }

    fn update(&mut self) {
        // ...
    }

    fn render_triangle(&mut self, frame: &wgpu::SwapChainOutput, encoder: &mut wgpu::CommandEncoder) {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor{
            color_attachments: &[
                wgpu::RenderPassColorAttachmentDescriptor {
                    attachment: &frame.view,
                    resolve_target: None,
                    load_op: wgpu::LoadOp::Clear,
                    store_op: wgpu::StoreOp::Store,
                    clear_color: self.clear_color,
                },
            ],
            depth_stencil_attachment: None,
        });

        render_pass.set_pipeline(&self.render_pipeline);
        //render_pass.set_bind_group(0, &self.diffuse_bind_group, &[]);
        render_pass.set_vertex_buffer(0, &self.vertex_buffer, 0, 0);
        render_pass.set_index_buffer(&self.index_buffer, 0, 0);
        render_pass.draw_indexed(0..self.num_indices, 0, 0..1);
    }

    // render ui
    fn render_imgui(&mut self, frame: &wgpu::SwapChainOutput, mut encoder: &mut wgpu::CommandEncoder, mut window: &mut glfw::Window) {
        self.imgui.platform
            .prepare_frame(self.imgui.context.io_mut(), &mut window)
            .expect("prepare_frame failed");

        let ui = self.imgui.context.frame();
        ui.show_demo_window(&mut false);

        let cursor = ui.mouse_cursor();
        if self.imgui.last_cursor != cursor {
            self.imgui.last_cursor = cursor;
            self.imgui.platform.prepare_render(&ui, &mut window)
        }

        self.imgui.renderer
            .render(ui.render(), &self.device, &mut encoder, &frame.view)
            .expect("render failed");
    }
    //

    fn render(&mut self, window: &mut glfw::Window) {
        let frame = self.swap_chain.get_next_texture()
            .expect("Timeout getting texture");

        let mut encoder = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("Render Encoder") }
        );

        self.render_triangle(&frame, &mut encoder);
        if self.imgui.should_render {
            self.render_imgui(&frame, &mut encoder, window);
        }

        self.queue.submit(&[encoder.finish()]);
    }
}

