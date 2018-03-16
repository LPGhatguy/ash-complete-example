#[macro_use]
extern crate ash;
extern crate cgmath;
extern crate winapi;
extern crate winit;

#[macro_use]
mod cstr;

mod context;

use std::default::Default;
use std::ptr;

use ash::vk;
use ash::version::{DeviceV1_0, EntryV1_0, InstanceV1_0, V1_0};
use ash::extensions;

use context::VulkanContext;

// Rust lets us statically embed built shaders straight into our binary!
static VERTEX_SHADER: &'static [u8] = include_bytes!("../built-shaders/triangle-vert.spv");
static FRAGMENT_SHADER: &'static [u8] = include_bytes!("../built-shaders/triangle-frag.spv");

// Our shaders all use the entrypoint 'main'
const SHADER_ENTRYPOINT_NAME: *const i8 = cstr!("main");

struct RandomGarbage {
    context: VulkanContext,
    window_size: (u32, u32),

    swapchain: vk::SwapchainKHR,
    swapchain_images: Vec<vk::Image>,
    swapchain_image_views: Vec<vk::ImageView>,

    shader_modules: Vec<vk::ShaderModule>,
    shader_stages: Vec<vk::PipelineShaderStageCreateInfo>,

    pipeline_layout: vk::PipelineLayout,
    render_pass: vk::RenderPass,
    graphics_pipeline: vk::Pipeline,

    swapchain_framebuffers: Vec<vk::Framebuffer>,

    command_pool: vk::CommandPool,
    command_buffers: Vec<vk::CommandBuffer>,

    image_available_semaphore: vk::Semaphore,
    render_finished_semaphore: vk::Semaphore,
}

impl RandomGarbage {
    fn new(context: VulkanContext, window_size: (u32, u32)) -> RandomGarbage {
        RandomGarbage {
            context,
            window_size,

            swapchain: vk::SwapchainKHR::null(),
            swapchain_images: Vec::new(),
            swapchain_image_views: Vec::new(),

            shader_modules: Vec::new(),
            shader_stages: Vec::new(),

            pipeline_layout: vk::PipelineLayout::null(),
            render_pass: vk::RenderPass::null(),
            graphics_pipeline: vk::Pipeline::null(),

            swapchain_framebuffers: Vec::new(),

            command_pool: vk::CommandPool::null(),
            command_buffers: Vec::new(),

            image_available_semaphore: vk::Semaphore::null(),
            render_finished_semaphore: vk::Semaphore::null(),
        }
    }

    fn query_surface_parameters(&self) -> SurfaceParameters {
        let surface_formats = self.context.surface_extension
            .get_physical_device_surface_formats_khr(self.context.physical_device, self.context.surface)
            .expect("Failed to query supported surface formats!");

        // Blindly pick the first surface format the system reports as supported.
        // Is this a good idea? Not really.
        let surface_format = surface_formats
            .get(0)
            .expect("Unable to find a surface format!");

        let surface_capabilities = self.context.surface_extension
            .get_physical_device_surface_capabilities_khr(self.context.physical_device, self.context.surface)
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
                width: self.window_size.0,
                height: self.window_size.1,
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
}

fn main() {
    let (window_width, window_height) = (800, 600);

    let flexo = VulkanContext::new();
    let mut burgers = RandomGarbage::new(flexo, (window_width, window_height));

    let surface_parameters = burgers.query_surface_parameters();

    burgers.swapchain = create_swapchain(
        &burgers.context.surface_extension,
        &burgers.context.swapchain_extension,
        burgers.context.physical_device,
        burgers.context.surface,
        &surface_parameters,
    );

    // Pull our list of images out from the swapchain, we'll need these later.
    burgers.swapchain_images = burgers.context.swapchain_extension.get_swapchain_images_khr(burgers.swapchain)
        .expect("Unable to get swapchain images!");

    burgers.swapchain_image_views = create_swapchain_image_views(
        &burgers.context.device,
        &burgers.swapchain_images,
        surface_parameters.format,
    );

    {
        let (shader_modules, shader_stages) = create_shaders(&burgers.context.device);

        burgers.shader_modules = shader_modules;
        burgers.shader_stages = shader_stages;
    }

    {
        let (pipeline_layout, render_pass, graphics_pipeline) = create_graphics_pipeline(
            &burgers.context.device,
            &burgers.shader_stages,
            surface_parameters.resolution,
            surface_parameters.format,
        );

        burgers.pipeline_layout = pipeline_layout;
        burgers.render_pass = render_pass;
        burgers.graphics_pipeline = graphics_pipeline;
    }

    burgers.swapchain_framebuffers = create_swapchain_framebuffers(
        &burgers.context.device,
        &burgers.swapchain_image_views,
        burgers.render_pass,
        surface_parameters.resolution,
    );

    burgers.command_pool = create_command_pool(&burgers.context.device, burgers.context.the_queue);

    burgers.command_buffers = create_command_buffers(
        &burgers.context.device,
        burgers.command_pool,
        &burgers.swapchain_framebuffers,
        burgers.render_pass,
        &surface_parameters,
        burgers.graphics_pipeline,
    );

    {
        let (image_available_semaphore, render_finished_semaphore) = create_semaphores(&burgers.context.device);

        burgers.image_available_semaphore = image_available_semaphore;
        burgers.render_finished_semaphore = render_finished_semaphore;
    }

    // It's main loop time!
    loop {
        let mut quit = false;
        burgers.context.events_loop.poll_events(|event| {
            match event {
                winit::Event::WindowEvent { event: winit::WindowEvent::Closed, .. } => {
                    quit = true;
                },
                winit::Event::WindowEvent { event: winit::WindowEvent::Resized(width, height), ..} => {
                    println!("Window resized to {}, {}", width, height);
                },
                _ => ()
            }
        });

        if quit {
            break;
        }

        let present_queue = unsafe {
            burgers.context.device.get_device_queue(burgers.context.the_queue, 0)
        };

        render_frame(
            &burgers.context.device,
            &burgers.context.swapchain_extension,
            burgers.swapchain,
            burgers.image_available_semaphore,
            burgers.render_finished_semaphore,
            &burgers.command_buffers,
            present_queue,
        );
    }

    burgers.context.device.device_wait_idle()
        .expect("Unable to wait for device to idle? (huh)");

    // Make sure you clean up after yourself!
    unsafe {
        burgers.context.device.destroy_semaphore(burgers.image_available_semaphore, None);
        burgers.context.device.destroy_semaphore(burgers.render_finished_semaphore, None);

        burgers.context.device.destroy_command_pool(burgers.command_pool, None);

        for &framebuffer in &burgers.swapchain_framebuffers {
            burgers.context.device.destroy_framebuffer(framebuffer, None);
        }

        burgers.context.device.destroy_pipeline(burgers.graphics_pipeline, None);
        burgers.context.device.destroy_render_pass(burgers.render_pass, None);
        burgers.context.device.destroy_pipeline_layout(burgers.pipeline_layout, None);

        for &shader_module in &burgers.shader_modules {
            burgers.context.device.destroy_shader_module(shader_module, None);
        }

        for &image_view in &burgers.swapchain_image_views {
            burgers.context.device.destroy_image_view(image_view, None);
        }

        burgers.context.swapchain_extension.destroy_swapchain_khr(burgers.swapchain, None);
    }
}

///
fn create_shaders<D>(
    device: &D,
) -> (Vec<vk::ShaderModule>, Vec<vk::PipelineShaderStageCreateInfo>)
    where D: DeviceV1_0
{
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
            device.create_shader_module(&create_info, None)
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
            device.create_shader_module(&create_info, None)
                .expect("Unable to create fragment shader module!")
        };

        shader_module
    };

    let shader_modules = vec![vertex_shader_module, fragment_shader_module];

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

    let shader_stages = vec![vertex_stage_info, fragment_stage_info];

    (shader_modules, shader_stages)
}

fn create_graphics_pipeline<D>(
    device: &D,
    shader_stages: &[vk::PipelineShaderStageCreateInfo],
    surface_resolution: vk::Extent2D,
    surface_format: vk::Format,
) -> (vk::PipelineLayout, vk::RenderPass, vk::Pipeline)
    where D: DeviceV1_0
{
    // Next, we need to describe what our vertex data looks like.
    // Hint: there isn't any!
    let vertex_input_state = vk::PipelineVertexInputStateCreateInfo {
        s_type: vk::StructureType::PipelineVertexInputStateCreateInfo,
        p_next: ptr::null(),
        flags: Default::default(),
        vertex_binding_description_count: 0,
        p_vertex_binding_descriptions: ptr::null(),
        vertex_attribute_description_count: 0,
        p_vertex_attribute_descriptions: ptr::null(),
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
        width: surface_resolution.width as f32,
        height: surface_resolution.height as f32,
        min_depth: 0.0,
        max_depth: 0.0,
    };

    let scissor = vk::Rect2D {
        offset: vk::Offset2D {
            x: 0,
            y: 0,
        },
        extent: surface_resolution,
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

    let pipeline_layout = unsafe {
        device.create_pipeline_layout(&pipeline_layout_info, None)
            .expect("Unable to create pipeline layout!")
    };

    // Create a color attachment to use our swapchain in our render pass.
    let color_attachment = vk::AttachmentDescription {
        flags: Default::default(),
        format: surface_format,
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

    let render_pass = unsafe {
        device.create_render_pass(&render_pass_info, None)
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
        stage_count: shader_stages.len() as u32,
        p_stages: shader_stages.as_ptr(),
        p_vertex_input_state: &vertex_input_state,
        p_input_assembly_state: &input_assembly_state,
        p_viewport_state: &viewport_state,
        p_rasterization_state: &rasterization_state,
        p_multisample_state: &multisample_state,
        p_depth_stencil_state: ptr::null(),
        p_color_blend_state: &color_blend_state,
        p_dynamic_state: ptr::null(),
        p_tessellation_state: ptr::null(),
        layout: pipeline_layout,
        render_pass: render_pass,
        subpass: 0,
        base_pipeline_handle: vk::Pipeline::null(),
        base_pipeline_index: -1,
    };

    let graphics_pipeline = unsafe {
        device.create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
            .expect("Unable to create graphics pipeline!")[0]
    };

    (pipeline_layout, render_pass, graphics_pipeline)
}

fn create_swapchain_image_views<D>(
    device: &D,
    swapchain_images: &[vk::Image],
    surface_format: vk::Format,
) -> Vec<vk::ImageView>
    where D: DeviceV1_0
{
    // To use our swapchain images, we need to construct image views that
    // describe how to map color channels, access, etc.
    let swapchain_image_views = swapchain_images
        .iter()
        .map(|&swapchain_image| {
            let create_info = vk::ImageViewCreateInfo {
                s_type: vk::StructureType::ImageViewCreateInfo,
                p_next: ptr::null(),
                flags: Default::default(),
                image: swapchain_image,
                view_type: vk::ImageViewType::Type2d,
                format: surface_format,
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
                device.create_image_view(&create_info, None)
                    .expect("Failed to create image view for swapchain image!")
            };

            image_view
        })
        .collect::<Vec<_>>();

    swapchain_image_views
}

fn create_swapchain_framebuffers<D>(
    device: &D,
    swapchain_image_views: &[vk::ImageView],
    render_pass: vk::RenderPass,
    surface_resolution: vk::Extent2D,
) -> Vec<vk::Framebuffer>
    where D: DeviceV1_0
{
    // Create a framebuffer object for each image in our swapchain!
    let swapchain_framebuffers = swapchain_image_views
        .iter()
        .map(|&image_view| {
            let framebuffer_info = vk::FramebufferCreateInfo {
                s_type: vk::StructureType::FramebufferCreateInfo,
                p_next: ptr::null(),
                flags: Default::default(),
                render_pass: render_pass,
                attachment_count: 1,
                p_attachments: &image_view,
                width: surface_resolution.width,
                height: surface_resolution.height,
                layers: 1,
            };

            let framebuffer = unsafe {
                device.create_framebuffer(&framebuffer_info, None)
                    .expect("Unable to create framebuffer!")
            };

            framebuffer
        })
        .collect::<Vec<_>>();

    swapchain_framebuffers
}

struct SurfaceParameters {
    resolution: vk::Extent2D,
    format: vk::Format,
    color_space: vk::ColorSpaceKHR,
    swapchain_image_count: u32,
    capabilities: vk::SurfaceCapabilitiesKHR
}

fn create_swapchain(
    surface_extension: &extensions::Surface,
    swapchain_extension: &extensions::Swapchain,
    physical_device: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
    surface_parameters: &SurfaceParameters,
) -> vk::SwapchainKHR
{
    let present_modes = surface_extension
        .get_physical_device_surface_present_modes_khr(physical_device, surface)
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
        surface: surface,
        min_image_count: surface_parameters.swapchain_image_count,
        image_color_space: surface_parameters.color_space,
        image_format: surface_parameters.format,
        image_extent: surface_parameters.resolution,
        image_array_layers: 1,
        image_usage: vk::IMAGE_USAGE_COLOR_ATTACHMENT_BIT,
        image_sharing_mode: vk::SharingMode::Exclusive,
        queue_family_index_count: 0,
        p_queue_family_indices: ptr::null(),
        pre_transform: surface_parameters.capabilities.current_transform,
        composite_alpha: vk::COMPOSITE_ALPHA_OPAQUE_BIT_KHR,
        present_mode: present_mode,
        clipped: 1,
        old_swapchain: vk::SwapchainKHR::null(),
    };

    // After a long-winded setup, actually create our swapchain
    let swapchain = unsafe {
        swapchain_extension
            .create_swapchain_khr(&swapchain_create_info, None)
            .expect("Unable to create swapchain!")
    };

    swapchain
}

fn create_command_pool<D>(
    device: &D,
    queue_family_index: u32,
) -> vk::CommandPool
    where D: DeviceV1_0
{
    // Create a command pool to allocate our command buffers from.
    let command_pool_info = vk::CommandPoolCreateInfo {
        s_type: vk::StructureType::CommandPoolCreateInfo,
        p_next: ptr::null(),
        flags: Default::default(),
        queue_family_index: queue_family_index,
    };

    let command_pool = unsafe {
        device.create_command_pool(&command_pool_info, None)
            .expect("Unable to create command pool!")
    };

    command_pool
}

fn create_command_buffers<D>(
    device: &D,
    command_pool: vk::CommandPool,
    swapchain_framebuffers: &[vk::Framebuffer],
    render_pass: vk::RenderPass,
    surface_parameters: &SurfaceParameters,
    graphics_pipeline: vk::Pipeline,
) -> Vec<vk::CommandBuffer>
    where D: DeviceV1_0
{
    let command_buffers_info = vk::CommandBufferAllocateInfo {
        s_type: vk::StructureType::CommandBufferAllocateInfo,
        p_next: ptr::null(),
        command_pool: command_pool,
        level: vk::CommandBufferLevel::Primary,
        command_buffer_count: swapchain_framebuffers.len() as u32,
    };

    let command_buffers = unsafe {
        device.allocate_command_buffers(&command_buffers_info)
            .expect("Unable to allocate command buffers!")
    };

    for (index, &command_buffer) in command_buffers.iter().enumerate() {
        let begin_info = vk::CommandBufferBeginInfo {
            s_type: vk::StructureType::CommandBufferBeginInfo,
            p_next: ptr::null(),
            flags: vk::COMMAND_BUFFER_USAGE_SIMULTANEOUS_USE_BIT,
            p_inheritance_info: ptr::null(),
        };

        unsafe {
            device.begin_command_buffer(command_buffer, &begin_info)
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
            render_pass: render_pass,
            framebuffer: swapchain_framebuffers[index],
            render_area: vk::Rect2D {
                offset: vk::Offset2D {
                    x: 0,
                    y: 0,
                },
                extent: surface_parameters.resolution,
            },
            clear_value_count: 1,
            p_clear_values: &clear_color,
        };

        unsafe {
            device.cmd_begin_render_pass(command_buffer, &render_pass_info, vk::SubpassContents::Inline);
            device.cmd_bind_pipeline(command_buffer, vk::PipelineBindPoint::Graphics, graphics_pipeline);
            device.cmd_draw(command_buffer,
                3, // vertex_count
                1, // instance_count
                0, // first_vertex
                0, // first_instance
            );
            device.cmd_end_render_pass(command_buffer);

            device.end_command_buffer(command_buffer)
                .expect("Unable to end command buffer!");
        }
    }

    command_buffers
}

fn create_semaphores<D>(
    device: &D,
) -> (vk::Semaphore, vk::Semaphore)
    where D: DeviceV1_0
{
    let semaphore_info = vk::SemaphoreCreateInfo {
        s_type: vk::StructureType::SemaphoreCreateInfo,
        p_next: ptr::null(),
        flags: Default::default(),
    };

    let image_available_semaphore = unsafe {
        device.create_semaphore(&semaphore_info, None)
            .expect("Unable to create semaphore!")
    };

    let render_finished_semaphore = unsafe {
        device.create_semaphore(&semaphore_info, None)
            .expect("Unable to create semaphore!")
    };

    (image_available_semaphore, render_finished_semaphore)
}

fn render_frame<D>(
    device: &D,
    swapchain_extension: &extensions::Swapchain,
    swapchain: vk::SwapchainKHR,
    image_available_semaphore: vk::Semaphore,
    render_finished_semaphore: vk::Semaphore,
    command_buffers: &[vk::CommandBuffer],
    present_queue: vk::Queue,
)
    where D: DeviceV1_0
{
    let image_index = unsafe {
        swapchain_extension.acquire_next_image_khr(swapchain, std::u64::MAX, image_available_semaphore, vk::Fence::null())
            .expect("Unable to acquire next swapchain image!")
    };

    let wait_stages = [vk::PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT];

    let submit_info = vk::SubmitInfo {
        s_type: vk::StructureType::SubmitInfo,
        p_next: ptr::null(),
        wait_semaphore_count: 1,
        p_wait_semaphores: &image_available_semaphore,
        p_wait_dst_stage_mask: wait_stages.as_ptr(),
        signal_semaphore_count: 1,
        p_signal_semaphores: &render_finished_semaphore,
        command_buffer_count: 1,
        p_command_buffers: &command_buffers[image_index as usize],
    };

    unsafe {
        device.queue_submit(present_queue, &[submit_info], vk::Fence::null())
            .expect("Unable to submit to queue!");
    }

    let present_info = vk::PresentInfoKHR {
        s_type: vk::StructureType::PresentInfoKhr,
        p_next: ptr::null(),
        wait_semaphore_count: 1,
        p_wait_semaphores: &render_finished_semaphore,
        swapchain_count: 1,
        p_swapchains: &swapchain,
        p_image_indices: &image_index,
        p_results: ptr::null_mut(),
    };

    unsafe {
        swapchain_extension.queue_present_khr(present_queue, &present_info)
            .expect("Unable to present!");
    }
}