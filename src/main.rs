#[macro_use] extern crate ash;
extern crate cgmath;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate memoffset;
extern crate winapi;
extern crate winit;

#[macro_use]
mod cstr;
mod context;
mod vertex;

use std::default::Default;
use std::ptr;
use std::mem;

use ash::vk;
use ash::version::{DeviceV1_0, InstanceV1_0};

use cgmath::{Vector2, Vector3};

use vertex::Vertex;
use context::VulkanContext;

// Rust lets us statically embed built shaders straight into our binary!
static VERTEX_SHADER: &'static [u8] = include_bytes!(concat!(env!("OUT_DIR"), "/triangle-vert.spv"));
static FRAGMENT_SHADER: &'static [u8] = include_bytes!(concat!(env!("OUT_DIR"), "/triangle-frag.spv"));

// Our shaders all use the entrypoint 'main'
const SHADER_ENTRYPOINT_NAME: *const i8 = cstr!("main");

lazy_static! {
    static ref TRIANGLE_VERTICES: Vec<Vertex> = vec![
        Vertex {
            position: Vector2::new(0.0, -0.5),
            color: Vector3::new(1.0, 0.0, 0.0),
        },
        Vertex {
            position: Vector2::new(0.5, 0.5),
            color: Vector3::new(0.0, 1.0, 0.0),
        },
        Vertex {
            position: Vector2::new(-0.5, 0.5),
            color: Vector3::new(0.0, 0.0, 1.0),
        },
    ];
}

struct SurfaceParameters {
    resolution: vk::Extent2D,
    format: vk::Format,
    color_space: vk::ColorSpaceKHR,
    swapchain_image_count: u32,
    capabilities: vk::SurfaceCapabilitiesKHR,
}

struct TwoStrokeApp {
    context: VulkanContext,
    window_size: (u32, u32),
    surface_parameters: SurfaceParameters,

    swapchain: vk::SwapchainKHR,
    swapchain_images: Vec<vk::Image>,
    swapchain_image_views: Vec<vk::ImageView>,

    shader_modules: Vec<vk::ShaderModule>,
    shader_stages: Vec<vk::PipelineShaderStageCreateInfo>,

    pipeline_layout: vk::PipelineLayout,
    render_pass: vk::RenderPass,
    graphics_pipeline: vk::Pipeline,

    swapchain_framebuffers: Vec<vk::Framebuffer>,

    vertex_buffer: vk::Buffer,
    vertex_buffer_memory: vk::DeviceMemory,

    command_pool: vk::CommandPool,
    command_buffers: Vec<vk::CommandBuffer>,

    image_available_semaphore: vk::Semaphore,
    render_finished_semaphore: vk::Semaphore,
}

impl TwoStrokeApp {
    fn new(context: VulkanContext, window_size: (u32, u32)) -> TwoStrokeApp {
        let surface_parameters = TwoStrokeApp::query_surface_parameters(&context, window_size);

        TwoStrokeApp {
            context,
            window_size,
            surface_parameters,

            swapchain: vk::SwapchainKHR::null(),
            swapchain_images: Vec::new(),
            swapchain_image_views: Vec::new(),

            shader_modules: Vec::new(),
            shader_stages: Vec::new(),

            pipeline_layout: vk::PipelineLayout::null(),
            render_pass: vk::RenderPass::null(),
            graphics_pipeline: vk::Pipeline::null(),

            swapchain_framebuffers: Vec::new(),

            vertex_buffer: vk::Buffer::null(),
            vertex_buffer_memory: vk::DeviceMemory::null(),

            command_pool: vk::CommandPool::null(),
            command_buffers: Vec::new(),

            image_available_semaphore: vk::Semaphore::null(),
            render_finished_semaphore: vk::Semaphore::null(),
        }
    }

    fn query_surface_parameters(context: &VulkanContext, window_size: (u32, u32)) -> SurfaceParameters {
        let surface_formats = context.surface_extension
            .get_physical_device_surface_formats_khr(context.physical_device, context.surface)
            .expect("Failed to query supported surface formats!");

        // Blindly pick the first surface format the system reports as supported.
        // Is this a good idea? Not really.
        let surface_format = surface_formats
            .get(0)
            .expect("Unable to find a surface format!");

        let surface_capabilities = context.surface_extension
            .get_physical_device_surface_capabilities_khr(context.physical_device, context.surface)
            .expect("Unable to query surface capabilities!");

        // Use the minimum number of images that our surface supports, plus one to
        // handle triple-buffering correctly.
        let mut desired_image_count = surface_capabilities.min_image_count + 1;

        // If max_image_count is 0, that means the implementation has no limit.
        //
        // Here, we make sure that we don't exceed the maximum!
        if surface_capabilities.max_image_count > 0 && desired_image_count > surface_capabilities.max_image_count {
            desired_image_count = surface_capabilities.max_image_count;
        }

        // If current_extent is (u32::MAX, u32::MAX), the size of the surface
        // is determined by the swapchain.
        let surface_resolution = match surface_capabilities.current_extent.width {
            std::u32::MAX => vk::Extent2D {
                width: window_size.0,
                height: window_size.1,
            },
            _ => surface_capabilities.current_extent,
        };

        SurfaceParameters {
            resolution: surface_resolution,
            format: surface_format.format,
            color_space: surface_format.color_space,
            swapchain_image_count: desired_image_count,
            capabilities: surface_capabilities,
        }
    }

    fn create_swapchain(&mut self) {
        let present_modes = self.context.surface_extension
            .get_physical_device_surface_present_modes_khr(self.context.physical_device, self.context.surface)
            .expect("Unable to query surface present modes!");

        // We prefer to use Mailbox mode for presenting, but if it isn't available,
        // fall back to Fifo, which is guaranteed by the spec to be supported.
        let present_mode = present_modes
            .iter()
            .cloned()
            .find(|&mode| mode == vk::PresentModeKHR::Mailbox)
            .unwrap_or(vk::PresentModeKHR::Fifo);

        // Swapchains need a *lot* of information.
        let swapchain_create_info = vk::SwapchainCreateInfoKHR {
            s_type: vk::StructureType::SwapchainCreateInfoKhr,
            p_next: ptr::null(),
            flags: Default::default(),
            surface: self.context.surface,
            min_image_count: self.surface_parameters.swapchain_image_count,
            image_color_space: self.surface_parameters.color_space,
            image_format: self.surface_parameters.format,
            image_extent: self.surface_parameters.resolution,
            image_array_layers: 1,
            image_usage: vk::IMAGE_USAGE_COLOR_ATTACHMENT_BIT,
            image_sharing_mode: vk::SharingMode::Exclusive,
            queue_family_index_count: 0,
            p_queue_family_indices: ptr::null(),
            pre_transform: self.surface_parameters.capabilities.current_transform,
            composite_alpha: vk::COMPOSITE_ALPHA_OPAQUE_BIT_KHR,
            present_mode: present_mode,
            clipped: 1,
            old_swapchain: vk::SwapchainKHR::null(),
        };

        // After a long-winded setup, actually create our swapchain
        self.swapchain = unsafe {
            self.context.swapchain_extension
                .create_swapchain_khr(&swapchain_create_info, None)
                .expect("Unable to create swapchain!")
        };
    }

    fn create_swapchain_images(&mut self) {
        // Pull our list of images out from the swapchain, we'll need these later.
        self.swapchain_images = self.context.swapchain_extension.get_swapchain_images_khr(self.swapchain)
            .expect("Unable to get swapchain images!");
    }

    fn create_swapchain_image_views(&mut self) {
        // To use our swapchain images, we need to construct image views that
        // describe how to map color channels, access, etc.
        self.swapchain_image_views = self.swapchain_images
            .iter()
            .map(|&swapchain_image| {
                let create_info = vk::ImageViewCreateInfo {
                    s_type: vk::StructureType::ImageViewCreateInfo,
                    p_next: ptr::null(),
                    flags: Default::default(),
                    image: swapchain_image,
                    view_type: vk::ImageViewType::Type2d,
                    format: self.surface_parameters.format,
                    components: vk::ComponentMapping {
                        r: vk::ComponentSwizzle::Identity,
                        g: vk::ComponentSwizzle::Identity,
                        b: vk::ComponentSwizzle::Identity,
                        a: vk::ComponentSwizzle::Identity,
                    },
                    subresource_range: vk::ImageSubresourceRange {
                        aspect_mask: vk::IMAGE_ASPECT_COLOR_BIT,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    },
                };

                let image_view = unsafe {
                    self.context.device.create_image_view(&create_info, None)
                        .expect("Failed to create image view for swapchain image!")
                };

                image_view
            })
            .collect::<Vec<_>>();
    }

    fn create_shaders(&mut self) {
        // Create our vertex and fragment shader modules.
        let vertex_shader_module = {
            let create_info = vk::ShaderModuleCreateInfo {
                s_type: vk::StructureType::ShaderModuleCreateInfo,
                p_next: ptr::null(),
                flags: Default::default(),
                code_size: VERTEX_SHADER.len(),
                p_code: VERTEX_SHADER.as_ptr() as *const u32,
            };

            let shader_module = unsafe {
                self.context.device.create_shader_module(&create_info, None)
                    .expect("Unable to create vertex shader module!")
            };

            shader_module
        };

        let fragment_shader_module = {
            let create_info = vk::ShaderModuleCreateInfo {
                s_type: vk::StructureType::ShaderModuleCreateInfo,
                p_next: ptr::null(),
                flags: Default::default(),
                code_size: FRAGMENT_SHADER.len(),
                p_code: FRAGMENT_SHADER.as_ptr() as *const u32,
            };

            let shader_module = unsafe {
                self.context.device.create_shader_module(&create_info, None)
                    .expect("Unable to create fragment shader module!")
            };

            shader_module
        };

        self.shader_modules = vec![vertex_shader_module, fragment_shader_module];

        // Now, we'll link our dumb byte buffers (shader modules) together into
        // shader stages, which are a little bit smarter.

        let vertex_stage_info = vk::PipelineShaderStageCreateInfo {
            s_type: vk::StructureType::PipelineShaderStageCreateInfo,
            p_next: ptr::null(),
            flags: Default::default(),
            stage: vk::SHADER_STAGE_VERTEX_BIT,
            module: vertex_shader_module,
            p_name: SHADER_ENTRYPOINT_NAME,
            p_specialization_info: ptr::null(),
        };

        let fragment_stage_info = vk::PipelineShaderStageCreateInfo {
            s_type: vk::StructureType::PipelineShaderStageCreateInfo,
            p_next: ptr::null(),
            flags: Default::default(),
            stage: vk::SHADER_STAGE_FRAGMENT_BIT,
            module: fragment_shader_module,
            p_name: SHADER_ENTRYPOINT_NAME,
            p_specialization_info: ptr::null(),
        };

        self.shader_stages = vec![vertex_stage_info, fragment_stage_info];
    }

    fn create_graphics_pipeline(&mut self) {
        // Next, we need to describe what our vertex data looks like.
        let binding_description = Vertex::get_binding_description();
        let attribute_descriptions = Vertex::get_attribute_descriptions();

        let vertex_input_state = vk::PipelineVertexInputStateCreateInfo {
            s_type: vk::StructureType::PipelineVertexInputStateCreateInfo,
            p_next: ptr::null(),
            flags: Default::default(),
            vertex_binding_description_count: 1,
            p_vertex_binding_descriptions: &binding_description,
            vertex_attribute_description_count: 2,
            p_vertex_attribute_descriptions: attribute_descriptions.as_ptr(),
        };

        // What kind of geometry are we drawing today?
        let input_assembly_state = vk::PipelineInputAssemblyStateCreateInfo {
            s_type: vk::StructureType::PipelineInputAssemblyStateCreateInfo,
            p_next: ptr::null(),
            flags: Default::default(),
            topology: vk::PrimitiveTopology::TriangleList,
            primitive_restart_enable: vk::VK_FALSE,
        };

        // Define our viewport and scissor to create a viewport state!
        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: self.surface_parameters.resolution.width as f32,
            height: self.surface_parameters.resolution.height as f32,
            min_depth: 0.0,
            max_depth: 0.0,
        };

        let scissor = vk::Rect2D {
            offset: vk::Offset2D {
                x: 0,
                y: 0,
            },
            extent: self.surface_parameters.resolution,
        };

        let viewport_state = vk::PipelineViewportStateCreateInfo {
            s_type: vk::StructureType::PipelineViewportStateCreateInfo,
            p_next: ptr::null(),
            flags: Default::default(),
            viewport_count: 1,
            p_viewports: &viewport,
            scissor_count: 1,
            p_scissors: &scissor,
        };

        // Define rasterizer state, with things like depth testing and face culling.
        let rasterization_state = vk::PipelineRasterizationStateCreateInfo {
            s_type: vk::StructureType::PipelineRasterizationStateCreateInfo,
            p_next: ptr::null(),
            flags: Default::default(),
            depth_clamp_enable: vk::VK_FALSE,
            rasterizer_discard_enable: vk::VK_FALSE,
            polygon_mode: vk::PolygonMode::Fill,
            line_width: 1.0,
            cull_mode: vk::CULL_MODE_BACK_BIT,
            front_face: vk::FrontFace::Clockwise,
            depth_bias_enable: vk::VK_FALSE,
            depth_bias_constant_factor: 0.0,
            depth_bias_clamp: 0.0,
            depth_bias_slope_factor: 0.0,
        };

        // We don't want to multisampling, but we have to say so.
        let multisample_state = vk::PipelineMultisampleStateCreateInfo {
            s_type: vk::StructureType::PipelineMultisampleStateCreateInfo,
            p_next: ptr::null(),
            flags: Default::default(),
            sample_shading_enable: vk::VK_FALSE,
            rasterization_samples: vk::SAMPLE_COUNT_1_BIT,
            min_sample_shading: 1.0,
            p_sample_mask: ptr::null(),
            alpha_to_coverage_enable: vk::VK_FALSE,
            alpha_to_one_enable: vk::VK_FALSE,
        };

        // Specify color blending, currently turned off.
        let color_blend_attachment = vk::PipelineColorBlendAttachmentState {
            color_write_mask: vk::COLOR_COMPONENT_R_BIT | vk::COLOR_COMPONENT_G_BIT | vk::COLOR_COMPONENT_B_BIT |
                vk::COLOR_COMPONENT_A_BIT,
            blend_enable: vk::VK_FALSE,
            src_color_blend_factor: vk::BlendFactor::One,
            dst_color_blend_factor: vk::BlendFactor::Zero,
            color_blend_op: vk::BlendOp::Add,
            src_alpha_blend_factor: vk::BlendFactor::One,
            dst_alpha_blend_factor: vk::BlendFactor::Zero,
            alpha_blend_op: vk::BlendOp::Add,
        };

        let color_blend_state = vk::PipelineColorBlendStateCreateInfo {
            s_type: vk::StructureType::PipelineColorBlendStateCreateInfo,
            p_next: ptr::null(),
            flags: Default::default(),
            logic_op_enable: vk::VK_FALSE,
            logic_op: vk::LogicOp::Copy,
            attachment_count: 1,
            p_attachments: &color_blend_attachment,
            blend_constants: [0.0, 0.0, 0.0, 0.0],
        };

        let pipeline_layout_info = vk::PipelineLayoutCreateInfo {
            s_type: vk::StructureType::PipelineLayoutCreateInfo,
            p_next: ptr::null(),
            flags: Default::default(),
            set_layout_count: 0,
            p_set_layouts: ptr::null(),
            push_constant_range_count: 0,
            p_push_constant_ranges: ptr::null(),
        };

        self.pipeline_layout = unsafe {
            self.context.device.create_pipeline_layout(&pipeline_layout_info, None)
                .expect("Unable to create pipeline layout!")
        };

        // Create a color attachment to use our swapchain in our render pass.
        let color_attachment = vk::AttachmentDescription {
            flags: Default::default(),
            format: self.surface_parameters.format,
            samples: vk::SAMPLE_COUNT_1_BIT,
            load_op: vk::AttachmentLoadOp::Clear,
            store_op: vk::AttachmentStoreOp::Store,
            stencil_load_op: vk::AttachmentLoadOp::DontCare,
            stencil_store_op: vk::AttachmentStoreOp::DontCare,
            initial_layout: vk::ImageLayout::Undefined,
            final_layout: vk::ImageLayout::PresentSrcKhr,
        };

        let color_attachment_ref = vk::AttachmentReference {
            attachment: 0,
            layout: vk::ImageLayout::ColorAttachmentOptimal,
        };

        // Each render pass is comprised of one or more subpasses.
        let subpass = vk::SubpassDescription {
            flags: Default::default(),
            pipeline_bind_point: vk::PipelineBindPoint::Graphics,
            color_attachment_count: 1,
            p_color_attachments: &color_attachment_ref,
            p_resolve_attachments: ptr::null(),
            input_attachment_count: 0,
            p_input_attachments: ptr::null(),
            p_depth_stencil_attachment: ptr::null(),
            preserve_attachment_count: 0,
            p_preserve_attachments: ptr::null(),
        };

        let dependency = vk::SubpassDependency {
            dependency_flags: Default::default(),
            src_subpass: vk::VK_SUBPASS_EXTERNAL,
            dst_subpass: 0,
            src_stage_mask: vk::PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
            src_access_mask: vk::AccessFlags::empty(),
            dst_stage_mask: vk::PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
            dst_access_mask: vk::ACCESS_COLOR_ATTACHMENT_READ_BIT | vk::ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
        };

        let render_pass_info = vk::RenderPassCreateInfo {
            s_type: vk::StructureType::RenderPassCreateInfo,
            p_next: ptr::null(),
            flags: Default::default(),
            attachment_count: 1,
            p_attachments: &color_attachment,
            subpass_count: 1,
            p_subpasses: &subpass,
            dependency_count: 1,
            p_dependencies: &dependency,
        };

        self.render_pass = unsafe {
            self.context.device.create_render_pass(&render_pass_info, None)
                .expect("Failed to create render pass!")
        };

        // This is what the last hundreds of lines have been leading up to: actually
        // creating a graphics pipeline.
        //
        // At this point, we still haven't actually accomplished anything, though.
        let pipeline_info = vk::GraphicsPipelineCreateInfo {
            s_type: vk::StructureType::GraphicsPipelineCreateInfo,
            p_next: ptr::null(),
            flags: Default::default(),
            stage_count: self.shader_stages.len() as u32,
            p_stages: self.shader_stages.as_ptr(),
            p_vertex_input_state: &vertex_input_state,
            p_input_assembly_state: &input_assembly_state,
            p_viewport_state: &viewport_state,
            p_rasterization_state: &rasterization_state,
            p_multisample_state: &multisample_state,
            p_depth_stencil_state: ptr::null(),
            p_color_blend_state: &color_blend_state,
            p_dynamic_state: ptr::null(),
            p_tessellation_state: ptr::null(),
            layout: self.pipeline_layout,
            render_pass: self.render_pass,
            subpass: 0,
            base_pipeline_handle: vk::Pipeline::null(),
            base_pipeline_index: -1,
        };

        self.graphics_pipeline = unsafe {
            self.context.device.create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
                .expect("Unable to create graphics pipeline!")[0]
        };
    }

    fn create_swapchain_framebuffers(&mut self) {
        // Create a framebuffer object for each image in our swapchain!
        self.swapchain_framebuffers = self.swapchain_image_views
            .iter()
            .map(|&image_view| {
                let framebuffer_info = vk::FramebufferCreateInfo {
                    s_type: vk::StructureType::FramebufferCreateInfo,
                    p_next: ptr::null(),
                    flags: Default::default(),
                    render_pass: self.render_pass,
                    attachment_count: 1,
                    p_attachments: &image_view,
                    width: self.surface_parameters.resolution.width,
                    height: self.surface_parameters.resolution.height,
                    layers: 1,
                };

                let framebuffer = unsafe {
                    self.context.device.create_framebuffer(&framebuffer_info, None)
                        .expect("Unable to create framebuffer!")
                };

                framebuffer
            })
            .collect::<Vec<_>>();
    }

    fn create_vertex_buffer(&mut self) {
        let buffer_info = vk::BufferCreateInfo {
            s_type: vk::StructureType::BufferCreateInfo,
            p_next: ptr::null(),
            flags: vk::BufferCreateFlags::empty(),
            size: (mem::size_of::<Vertex>() * TRIANGLE_VERTICES.len()) as u64,
            usage: vk::BUFFER_USAGE_VERTEX_BUFFER_BIT,
            sharing_mode: vk::SharingMode::Exclusive,
            queue_family_index_count: 0,
            p_queue_family_indices: ptr::null(),
        };

        self.vertex_buffer = unsafe {
            self.context.device.create_buffer(&buffer_info, None)
                .expect("Unable to create vertex buffer!")
        };

        let memory_requirements = self.context.device.get_buffer_memory_requirements(self.vertex_buffer);

        let memory_type = self.find_memory_type(memory_requirements.memory_type_bits, vk::MEMORY_PROPERTY_HOST_VISIBLE_BIT | vk::MEMORY_PROPERTY_HOST_COHERENT_BIT)
            .expect("Unable to find suitable memory type!");

        let alloc_info = vk::MemoryAllocateInfo {
            s_type: vk::StructureType::MemoryAllocateInfo,
            p_next: ptr::null(),
            allocation_size: memory_requirements.size,
            memory_type_index: memory_type,
        };

        self.vertex_buffer_memory = unsafe {
            self.context.device.allocate_memory(&alloc_info, None)
                .expect("Unable to allocate memory!")
        };

        unsafe {
            self.context.device.bind_buffer_memory(self.vertex_buffer, self.vertex_buffer_memory, 0)
                .expect("Unable to bind buffer memory!");
        }

        unsafe {
            let mapped_memory = self.context.device.map_memory(self.vertex_buffer_memory, 0, memory_requirements.size, vk::MemoryMapFlags::empty())
                .expect("Unable to map memory!");

            let mut vertices = TRIANGLE_VERTICES.clone();
            ptr::copy(vertices.as_mut_ptr(), mapped_memory as *mut _, vertices.len());

            self.context.device.unmap_memory(self.vertex_buffer_memory);
        }
    }

    fn find_memory_type(&self, type_filter: u32, properties: vk::MemoryPropertyFlags) -> Option<u32> {
        let memory_properties = self.context.instance.get_physical_device_memory_properties(self.context.physical_device);

        for index in 0..memory_properties.memory_type_count {
            // We should only return memory in our type_filter
            if type_filter & (1 << index) == 0 {
                continue;
            }

            let memory = &memory_properties.memory_types[index as usize];
            if memory.property_flags == properties {
                return Some(index);
            }
        }

        None
    }

    fn create_command_pool(&mut self) {
        // Create a command pool to allocate our command buffers from.
        let command_pool_info = vk::CommandPoolCreateInfo {
            s_type: vk::StructureType::CommandPoolCreateInfo,
            p_next: ptr::null(),
            flags: Default::default(),
            queue_family_index: self.context.the_queue,
        };

        self.command_pool = unsafe {
            self.context.device.create_command_pool(&command_pool_info, None)
                .expect("Unable to create command pool!")
        };
    }

    fn create_command_buffers(&mut self) {
        let command_buffers_info = vk::CommandBufferAllocateInfo {
            s_type: vk::StructureType::CommandBufferAllocateInfo,
            p_next: ptr::null(),
            command_pool: self.command_pool,
            level: vk::CommandBufferLevel::Primary,
            command_buffer_count: self.swapchain_framebuffers.len() as u32,
        };

        self.command_buffers = unsafe {
            self.context.device.allocate_command_buffers(&command_buffers_info)
                .expect("Unable to allocate command buffers!")
        };

        for (index, &command_buffer) in self.command_buffers.iter().enumerate() {
            let begin_info = vk::CommandBufferBeginInfo {
                s_type: vk::StructureType::CommandBufferBeginInfo,
                p_next: ptr::null(),
                flags: vk::COMMAND_BUFFER_USAGE_SIMULTANEOUS_USE_BIT,
                p_inheritance_info: ptr::null(),
            };

            unsafe {
                self.context.device.begin_command_buffer(command_buffer, &begin_info)
                    .expect("Unable to begin command buffer!");
            }

            let clear_color = vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [0.39, 0.58, 0.93, 1.0],
                },
            };

            let render_pass_info = vk::RenderPassBeginInfo {
                s_type: vk::StructureType::RenderPassBeginInfo,
                p_next: ptr::null(),
                render_pass: self.render_pass,
                framebuffer: self.swapchain_framebuffers[index],
                render_area: vk::Rect2D {
                    offset: vk::Offset2D {
                        x: 0,
                        y: 0,
                    },
                    extent: self.surface_parameters.resolution,
                },
                clear_value_count: 1,
                p_clear_values: &clear_color,
            };

            unsafe {
                self.context.device.cmd_begin_render_pass(command_buffer, &render_pass_info, vk::SubpassContents::Inline);
                self.context.device.cmd_bind_pipeline(command_buffer, vk::PipelineBindPoint::Graphics, self.graphics_pipeline);

                self.context.device.cmd_bind_vertex_buffers(command_buffer, 0, &[self.vertex_buffer], &[0]);

                self.context.device.cmd_draw(command_buffer,
                    3, // vertex_count
                    1, // instance_count
                    0, // first_vertex
                    0, // first_instance
                );
                self.context.device.cmd_end_render_pass(command_buffer);

                self.context.device.end_command_buffer(command_buffer)
                    .expect("Unable to end command buffer!");
            }
        }
    }

    fn create_semaphores(&mut self) {
        let semaphore_info = vk::SemaphoreCreateInfo {
            s_type: vk::StructureType::SemaphoreCreateInfo,
            p_next: ptr::null(),
            flags: Default::default(),
        };

        self.image_available_semaphore = unsafe {
            self.context.device.create_semaphore(&semaphore_info, None)
                .expect("Unable to create semaphore!")
        };

        self.render_finished_semaphore = unsafe {
            self.context.device.create_semaphore(&semaphore_info, None)
                .expect("Unable to create semaphore!")
        };
    }

    fn render_frame(&mut self) {
        let present_queue = unsafe {
            self.context.device.get_device_queue(self.context.the_queue, 0)
        };

        let image_index = unsafe {
            let result = self.context.swapchain_extension.acquire_next_image_khr(
                self.swapchain,
                std::u64::MAX,
                self.image_available_semaphore,
                vk::Fence::null()
            );

            match result {
                Ok(v) => v,
                Err(vk::Result::ErrorOutOfDateKhr) | Err(vk::Result::SuboptimalKhr) => {
                    self.recreate_swapchain();
                    return;
                },
                Err(_) => panic!("Unable to acquire next image!"),
            }
        };

        let wait_stages = [vk::PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT];

        let submit_info = vk::SubmitInfo {
            s_type: vk::StructureType::SubmitInfo,
            p_next: ptr::null(),
            wait_semaphore_count: 1,
            p_wait_semaphores: &self.image_available_semaphore,
            p_wait_dst_stage_mask: wait_stages.as_ptr(),
            signal_semaphore_count: 1,
            p_signal_semaphores: &self.render_finished_semaphore,
            command_buffer_count: 1,
            p_command_buffers: &self.command_buffers[image_index as usize],
        };

        unsafe {
            self.context.device.queue_submit(present_queue, &[submit_info], vk::Fence::null())
                .expect("Unable to submit to queue!");
        }

        let present_info = vk::PresentInfoKHR {
            s_type: vk::StructureType::PresentInfoKhr,
            p_next: ptr::null(),
            wait_semaphore_count: 1,
            p_wait_semaphores: &self.render_finished_semaphore,
            swapchain_count: 1,
            p_swapchains: &self.swapchain,
            p_image_indices: &image_index,
            p_results: ptr::null_mut(),
        };

        unsafe {
            let result = self.context.swapchain_extension.queue_present_khr(present_queue, &present_info);

            match result {
                Ok(_) => (),
                Err(vk::Result::ErrorOutOfDateKhr) | Err(vk::Result::SuboptimalKhr) => {
                    self.recreate_swapchain();
                    return;
                },
                Err(_) => panic!("Unable to present!"),
            }
        }
    }

    fn cleanup_swapchain(&mut self) {
        unsafe {
            for &framebuffer in &self.swapchain_framebuffers {
                self.context.device.destroy_framebuffer(framebuffer, None);
            }

            self.context.device.free_command_buffers(self.command_pool, &self.command_buffers);
            self.command_buffers = Vec::new();

            self.context.device.destroy_pipeline(self.graphics_pipeline, None);
            self.context.device.destroy_render_pass(self.render_pass, None);
            self.context.device.destroy_pipeline_layout(self.pipeline_layout, None);

            for &image_view in &self.swapchain_image_views {
                self.context.device.destroy_image_view(image_view, None);
            }

            self.context.swapchain_extension.destroy_swapchain_khr(self.swapchain, None);
        }
    }

    fn recreate_swapchain(&mut self) {
        self.context.device.device_wait_idle()
            .expect("Unable to wait for device to idle!");

        self.cleanup_swapchain();

        self.create_swapchain();
        self.create_swapchain_images();
        self.create_swapchain_image_views();

        self.create_graphics_pipeline();

        self.create_swapchain_framebuffers();

        self.create_command_buffers();
    }

    fn cleanup(mut self) {
        self.context.device.device_wait_idle()
            .expect("Unable to wait for device to idle!");

        // Make sure you clean up after yourself!
        unsafe {
            self.context.device.destroy_semaphore(self.image_available_semaphore, None);
            self.context.device.destroy_semaphore(self.render_finished_semaphore, None);

            self.cleanup_swapchain();

            self.context.device.destroy_buffer(self.vertex_buffer, None);
            self.context.device.free_memory(self.vertex_buffer_memory, None);

            self.context.device.destroy_command_pool(self.command_pool, None);

            for &shader_module in &self.shader_modules {
                self.context.device.destroy_shader_module(shader_module, None);
            }
        }
    }
}

fn main() {
    let (window_width, window_height) = (800, 600);

    let mut the_app = TwoStrokeApp::new(VulkanContext::new(), (window_width, window_height));

    the_app.create_shaders();

    the_app.create_swapchain();
    the_app.create_swapchain_images();
    the_app.create_swapchain_image_views();

    the_app.create_graphics_pipeline();

    the_app.create_swapchain_framebuffers();

    the_app.create_vertex_buffer();

    the_app.create_command_pool();
    the_app.create_command_buffers();

    the_app.create_semaphores();

    // It's main loop time!
    loop {
        let mut quit = false;
        let mut resize_to = None;

        the_app.context.events_loop.poll_events(|event| {
            match event {
                winit::Event::WindowEvent { event: winit::WindowEvent::Closed, .. } => {
                    quit = true;
                },
                winit::Event::WindowEvent { event: winit::WindowEvent::Resized(width, height), ..} => {
                    resize_to = Some((width, height))
                },
                _ => ()
            }
        });

        if let Some(dimensions) = resize_to {
            the_app.window_size = dimensions;
            the_app.surface_parameters = TwoStrokeApp::query_surface_parameters(&the_app.context, dimensions);
            the_app.recreate_swapchain();
        }

        if quit {
            break;
        }

        the_app.render_frame();
    }

    the_app.cleanup();
}