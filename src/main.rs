#[macro_use]
extern crate ash;
extern crate cgmath;
extern crate winapi;
extern crate winit;

use std::default::Default;
use std::ffi::{CStr, CString};
use std::ptr;

use ash::{Entry, Instance, Device, vk};
use ash::version::{DeviceV1_0, EntryV1_0, InstanceV1_0, V1_0};
use ash::extensions;

// A handy little macro that lets us specify C-style strings
macro_rules! cstr {
    ($s:expr) => (
        concat!($s, "\0") as *const str as *const [i8] as *const i8
    );
}

// Rust lets us statically embed built shaders straight into our binary!
static VERTEX_SHADER: &'static [u8] = include_bytes!("../built-shaders/triangle-vert.spv");
static FRAGMENT_SHADER: &'static [u8] = include_bytes!("../built-shaders/triangle-frag.spv");

// Applications using Vulkan have to give Vulkan a name
const APP_NAME: *const i8 = cstr!("Ash Triangle");

// Our shaders all use the entrypoint 'main'
const SHADER_ENTRYPOINT_NAME: *const i8 = cstr!("main");

fn main() {
    let (window_width, window_height) = (800, 600);

    // Construct a regular winit events loop and window; nothing special here.
    let mut events_loop = winit::EventsLoop::new();
    let window = winit::WindowBuilder::new()
        .with_title("Ash Triangle")
        .with_dimensions(window_width, window_height)
        .build(&events_loop)
        .expect("Unable to construct winit window!");

    // 'Entry' implements a specific API version and automatically loads
    // function pointers for us.
    let entry = Entry::<V1_0>::new()
        .expect("Unable to create Vulkan Entry!");

    let instance = create_vulkan_instance(&entry);

    // Load VK_EXT_debug_report extension
    let debug_report_extension = extensions::DebugReport::new(&entry, &instance)
        .expect("Unable to load DebugReport extension");

    let debug_callback = set_up_debug_callback(&debug_report_extension);

    // Load VK_KHR_surface extension
    let surface_extension = extensions::Surface::new(&entry, &instance)
        .expect("Unable to load the Surface extension");

    let surface = create_surface(&entry, &instance, &window)
        .expect("Failed to create surface!");

    let (physical_device, queue_family_index) = choose_physical_device_and_queue_family(
        &instance,
        &surface_extension,
        surface
    );

    let device = create_logical_device(
        &instance,
        physical_device,
        queue_family_index,
    );

    // Pull the first queue from the family specified by queue_family_index out
    // of the device we just created.
    let present_queue = unsafe {
        device.get_device_queue(queue_family_index, 0)
    };

    let surface_parameters = query_surface_parameters(
        &surface_extension,
        physical_device,
        surface,
        (window_width, window_height),
    );

    // Load VK_KHR_swapchain extension
    let swapchain_extension = extensions::Swapchain::new(&instance, &device)
        .expect("Unable to load Swapchain extension!");

    let swapchain = create_swapchain(
        &surface_extension,
        &swapchain_extension,
        physical_device,
        surface,
        &surface_parameters,
    );

    // Pull our list of images out from the swapchain, we'll need these later.
    let swapchain_images = swapchain_extension.get_swapchain_images_khr(swapchain)
        .expect("Unable to get swapchain images!");

    let swapchain_image_views = create_swapchain_image_views(
        &device,
        &swapchain_images,
        surface_parameters.format,
    );

    let (shader_modules, shader_stages) = create_shaders(&device);

    let (pipeline_layout, render_pass, graphics_pipeline) = create_graphics_pipeline(
        &device,
        &shader_stages,
        surface_parameters.resolution,
        surface_parameters.format,
    );

    let swapchain_framebuffers = create_swapchain_framebuffers(
        &device,
        &swapchain_image_views,
        render_pass,
        surface_parameters.resolution,
    );

    let command_pool = create_command_pool(&device, queue_family_index);

    let command_buffers = create_command_buffers(
        &device,
        command_pool,
        &swapchain_framebuffers,
        render_pass,
        &surface_parameters,
        graphics_pipeline,
    );

    let (image_available_semaphore, render_finished_semaphore) = create_semaphores(&device);

    // It's main loop time!
    loop {
        let mut quit = false;
        events_loop.poll_events(|event| {
            match event {
                winit::Event::WindowEvent { event: winit::WindowEvent::Closed, .. } => {
                    quit = true;
                },
                _ => ()
            }
        });

        if quit {
            break;
        }

        render_frame(
            &device,
            &swapchain_extension,
            swapchain,
            image_available_semaphore,
            render_finished_semaphore,
            &command_buffers,
            present_queue,
        );
    }

    device.device_wait_idle()
        .expect("Unable to wait for device to idle? (huh)");

    // Make sure you clean up after yourself!
    unsafe {
        device.destroy_semaphore(image_available_semaphore, None);
        device.destroy_semaphore(render_finished_semaphore, None);

        device.destroy_command_pool(command_pool, None);

        for &framebuffer in &swapchain_framebuffers {
            device.destroy_framebuffer(framebuffer, None);
        }

        device.destroy_pipeline(graphics_pipeline, None);
        device.destroy_render_pass(render_pass, None);
        device.destroy_pipeline_layout(pipeline_layout, None);

        for &shader_module in &shader_modules {
            device.destroy_shader_module(shader_module, None);
        }

        for &image_view in &swapchain_image_views {
            device.destroy_image_view(image_view, None);
        }

        swapchain_extension.destroy_swapchain_khr(swapchain, None);

        device.destroy_device(None);

        surface_extension.destroy_surface_khr(surface, None);
        debug_report_extension.destroy_debug_report_callback_ext(debug_callback, None);

        instance.destroy_instance(None);
    }
}

// A set of platform-specific instance extensions.
//
// I don't have another machine to test other implementations, so only a Windows
// implementation is provided right now.
#[cfg(all(windows))]
fn instance_extension_names() -> Vec<*const i8> {
    vec![
        extensions::Surface::name().as_ptr(),
        extensions::DebugReport::name().as_ptr(),
        extensions::Win32Surface::name().as_ptr(),
    ]
}

// Uses a platform specific extension to create a surface. Like the
// extension_names() method, it's only implemented for Windows right now.
#[cfg(windows)]
fn create_surface(
    entry: &Entry<V1_0>,
    instance: &Instance<V1_0>,
    window: &winit::Window,
) -> Result<vk::SurfaceKHR, vk::Result> {
    use winapi::shared::windef::HWND;
    use winapi::um::winuser::GetWindow;
    use winit::os::windows::WindowExt;

    let hwnd = window.get_hwnd() as HWND;
    let hinstance = unsafe {
        GetWindow(hwnd, 0) as *const vk::c_void
    };

    let win32_create_info = vk::Win32SurfaceCreateInfoKHR {
        s_type: vk::StructureType::Win32SurfaceCreateInfoKhr,
        p_next: ptr::null(),
        flags: Default::default(),
        hinstance: hinstance,
        hwnd: hwnd as *const vk::c_void,
    };

    let win32_surface_extension = extensions::Win32Surface::new(entry, instance)
        .expect("Unable to load Win32Surface extension");

    unsafe {
        win32_surface_extension.create_win32_surface_khr(&win32_create_info, None)
    }
}

// The signature of this function is important -- we pass it to the debug
// callback extension below.
unsafe extern "system" fn vulkan_debug_callback(
    _flags: vk::DebugReportFlagsEXT,
    _obj_type: vk::DebugReportObjectTypeEXT,
    _obj: vk::uint64_t,
    _location: vk::size_t,
    _code: vk::int32_t,
    _layer_prefix: *const vk::c_char,
    p_message: *const vk::c_char,
    _user_data: *mut vk::c_void,
) -> u32 {
    println!("{:?}", CStr::from_ptr(p_message));

    vk::VK_FALSE
}

/// Creates a debug callback extension using the VK_EXT_debug_report extension.
fn set_up_debug_callback(debug_report_extension: &extensions::DebugReport) -> vk::DebugReportCallbackEXT {
    // Pick and choose what kind of debug messages we want to subscribe to and
    // pipe them to vulkan_debug_callback.
    let debug_info = vk::DebugReportCallbackCreateInfoEXT {
        s_type: vk::StructureType::DebugReportCallbackCreateInfoExt,
        p_next: ptr::null(),
        flags: vk::DEBUG_REPORT_ERROR_BIT_EXT | vk::DEBUG_REPORT_WARNING_BIT_EXT
            | vk::DEBUG_REPORT_PERFORMANCE_WARNING_BIT_EXT,
        pfn_callback: vulkan_debug_callback,
        p_user_data: ptr::null_mut(),
    };

    let debug_callback = unsafe {
        debug_report_extension
            .create_debug_report_callback_ext(&debug_info, None)
            .expect("Unable to attach DebugReport callback!")
    };

    debug_callback
}

/// Create a Vulkan instance using the given entrypoints.
///
/// Normally, functions should use trait bounds when accepting Entry and
/// Instance objects, but `create_vulkan_instance` needs a concrete Entry in
/// order to return a concrete Instance object, at least until impl trait is
/// stable.
fn create_vulkan_instance(entry: &Entry<V1_0>) -> Instance<V1_0> {
    // Right now, we unconditionally load the validation layers, which rely on
    // the LunarG Vulkan SDK being installed.
    let layer_names = [CString::new("VK_LAYER_LUNARG_standard_validation").unwrap()];
    let layers_names_raw: Vec<*const i8> = layer_names
        .iter()
        .map(|layer_name| layer_name.as_ptr())
        .collect();
    let extension_names_raw = instance_extension_names();

    let app_info = vk::ApplicationInfo {
        s_type: vk::StructureType::ApplicationInfo,
        p_next: ptr::null(),
        p_application_name: APP_NAME,
        application_version: 0,
        p_engine_name: APP_NAME,
        engine_version: 0,
        api_version: vk_make_version!(1, 0, 69),
    };

    let create_info = vk::InstanceCreateInfo {
        s_type: vk::StructureType::InstanceCreateInfo,
        p_next: ptr::null(),
        flags: Default::default(),
        p_application_info: &app_info,
        pp_enabled_layer_names: layers_names_raw.as_ptr(),
        enabled_layer_count: layers_names_raw.len() as u32,
        pp_enabled_extension_names: extension_names_raw.as_ptr(),
        enabled_extension_count: extension_names_raw.len() as u32,
    };

    let instance = unsafe {
        entry
            .create_instance(&create_info, None)
            .expect("Unable to create Vulkan instance")
    };

    instance
}

/// We need to locate a physical device available to the system that supports
/// all of the capabilities that we want.
///
/// This function returns a PhysicalDevice handle as well as a 'queue family index'
fn choose_physical_device_and_queue_family<I>(
    instance: &I,
    surface_extension: &extensions::Surface,
    surface: vk::SurfaceKHR,
) -> (vk::PhysicalDevice, u32)
    where I: InstanceV1_0,
{
    // Grab a list of physical devices we can use with our instance.
    let physical_devices = instance
        .enumerate_physical_devices()
        .expect("Failed to enumerate physical devices!");

    // For each physical device, attempt to locate a queue family that supports
    // all of the features we want.
    let (physical_device, queue_family_index) = physical_devices
        .iter()
        .filter_map(|physical_device| {
            let queue_families = instance.get_physical_device_queue_family_properties(*physical_device);

            queue_families
                .iter()
                .enumerate()
                .filter_map(|(index, info)| {
                    // Rust uses usize for array indexing, Vulkan uses u32.
                    let index = index as u32;

                    // We need a queue that supports graphics and the KHR
                    // surface extension.
                    let supports_graphics = info.queue_flags.subset(vk::QUEUE_GRAPHICS_BIT);

                    // Can this queue draw to the surface we made?
                    let supports_surface = surface_extension.get_physical_device_surface_support_khr(
                        *physical_device,
                        index,
                        surface,
                    );

                    if supports_graphics && supports_surface {
                        Some((*physical_device, index))
                    } else {
                        None
                    }
                })
                .nth(0)
        })
        .nth(0)
        .expect("Couldn't find suitable physical device that supports Vulkan and extensions!");

    (physical_device, queue_family_index)
}

/// Creates a logical device that represents a connection to a physical device
/// we've chosen.
///
/// We specify `queue_family_index`, since we need to know what kinds of queues
/// we want to use at device creation time.
fn create_logical_device(
    instance: &Instance<V1_0>,
    physical_device: vk::PhysicalDevice,
    queue_family_index: u32,
) -> Device<V1_0>
{
    // Our device needs to support the Swapchain extension.
    let device_extension_names_raw = [
        extensions::Swapchain::name().as_ptr(),
    ];

    // We don't specify any extra physical device features, but this is where
    // they'd go.
    let physical_device_features = vk::PhysicalDeviceFeatures {
        ..Default::default()
    };

    // We're creating one queue of type `queue_family_index`
    let queue_priorities = [1.0];
    let queue_info = vk::DeviceQueueCreateInfo {
        s_type: vk::StructureType::DeviceQueueCreateInfo,
        p_next: ptr::null(),
        flags: Default::default(),
        queue_family_index: queue_family_index,
        p_queue_priorities: queue_priorities.as_ptr(),
        queue_count: queue_priorities.len() as u32,
    };

    // Specify that we want to create a Device using one queue of a single queue
    // family, specified by queue_info above.
    let device_create_info = vk::DeviceCreateInfo {
        s_type: vk::StructureType::DeviceCreateInfo,
        p_next: ptr::null(),
        flags: Default::default(),
        queue_create_info_count: 1,
        p_queue_create_infos: &queue_info,
        enabled_layer_count: 0,
        pp_enabled_layer_names: ptr::null(),
        enabled_extension_count: device_extension_names_raw.len() as u32,
        pp_enabled_extension_names: device_extension_names_raw.as_ptr(),
        p_enabled_features: &physical_device_features,
    };

    // Create our logical device using our information above.
    let device: Device<V1_0> = unsafe {
        instance
            .create_device(physical_device, &device_create_info, None)
            .expect("Unable to create Device!")
    };

    device
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

fn query_surface_parameters(
    surface_extension: &extensions::Surface,
    physical_device: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
    window_size: (u32, u32),
) -> SurfaceParameters
{
    let surface_formats = surface_extension
        .get_physical_device_surface_formats_khr(physical_device, surface)
        .expect("Failed to query supported surface formats!");

    // Blindly pick the first surface format the system reports as supported.
    // Is this a good idea? Not really.
    let surface_format = surface_formats
        .get(0)
        .expect("Unable to find a surface format!");

    let surface_capabilities = surface_extension
        .get_physical_device_surface_capabilities_khr(physical_device, surface)
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