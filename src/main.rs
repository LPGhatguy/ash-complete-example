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
use ash::extensions::{DebugReport, Surface, Swapchain, Win32Surface};

// A set of platform-specific instance extensions.
//
// I don't have another machine to test other implementations, so only a Windows
// implementation is provided right now.
#[cfg(all(windows))]
fn extension_names() -> Vec<*const i8> {
    vec![
        Surface::name().as_ptr(),
        DebugReport::name().as_ptr(),
        Win32Surface::name().as_ptr(),
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

    let win32_surface_extension = Win32Surface::new(entry, instance)
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
    1
}

fn main() {
    let (window_width, window_height) = (800, 600);

    // Construct a regular winit events loop and window; nothing special here.
    let mut events_loop = winit::EventsLoop::new();
    let window = winit::WindowBuilder::new()
        .with_title("Try Ash")
        .with_dimensions(window_width, window_height)
        .build(&events_loop)
        .expect("Unable to construct winit window!");

    // 'Entry' implements a specific API version and automatically loads
    // function pointers for us.
    let entry = Entry::<V1_0>::new()
        .expect("Unable to create Vulkan Entry!");

    // Vulkan requires us to specify an app and engine name, so we use the same
    // one for both.
    let app_name = CString::new("TryAsh").unwrap();
    let raw_name = app_name.as_ptr();

    // Right now, we unconditionally load the validation layers, which rely on
    // the LunarG Vulkan SDK being installed.
    let layer_names = [CString::new("VK_LAYER_LUNARG_standard_validation").unwrap()];
    let layers_names_raw: Vec<*const i8> = layer_names
        .iter()
        .map(|layer_name| layer_name.as_ptr())
        .collect();
    let extension_names_raw = extension_names();

    let app_info = vk::ApplicationInfo {
        p_application_name: raw_name,
        s_type: vk::StructureType::ApplicationInfo,
        p_next: ptr::null(),
        application_version: 0,
        p_engine_name: raw_name,
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

    let debug_report_extension = DebugReport::new(&entry, &instance)
        .expect("Unable to load DebugReport extension");

    let debug_callback = unsafe {
        debug_report_extension
            .create_debug_report_callback_ext(&debug_info, None)
            .expect("Unable to attach DebugReport callback!")
    };

    let surface = create_surface(&entry, &instance, &window)
        .expect("Failed to create surface!");

    // Load the VK_KHR_Surface extension
    let surface_extension = Surface::new(&entry, &instance).expect("Unable to load the Surface extension");

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
        .expect("Couldn't find suitable physical device.");

    // Our device needs to support the Swapchain extension.
    let device_extension_names_raw = [Swapchain::name().as_ptr()];

    // We don't specify any extra device features, but this is where they'd go.
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

    // Create our device using our information above.
    let device: Device<V1_0> = unsafe {
        instance
            .create_device(physical_device, &device_create_info, None)
            .expect("Unable to create Device!")
    };

    // Pull the first queue from the family specified by queue_family_index out
    // of the device we just created.
    let present_queue = unsafe {
        device.get_device_queue(queue_family_index, 0)
    };

    let surface_formats = surface_extension
        .get_physical_device_surface_formats_khr(physical_device, surface)
        .expect("Failed to query supported surface formats!");

    // Blindly pick the first surface format the system reports as supported.
    // Is this a good idea? I don't know.
    let surface_format = surface_formats
        .get(0)
        .expect("Unable to find a surface format!");

    let surface_capabilities = surface_extension
        .get_physical_device_surface_capabilities_khr(physical_device, surface)
        .expect("Unable to query surface capabilities!");

    // Use the minimum number of images that our surface supports.
    let desired_image_count = surface_capabilities.min_image_count;

    // If current_extent is (u32::MAX, u32::MAX), the size of the surface
    // is determined by the swapchain.
    let surface_resolution = match surface_capabilities.current_extent.width {
        std::u32::MAX => vk::Extent2D {
            width: window_width,
            height: window_height,
        },
        _ => surface_capabilities.current_extent,
    };

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

    let swapchain_extension = Swapchain::new(&instance, &device)
        .expect("Unable to load Swapchain extension!");

    // Swapchains need a *lot* of information.
    let swapchain_create_info = vk::SwapchainCreateInfoKHR {
        s_type: vk::StructureType::SwapchainCreateInfoKhr,
        p_next: ptr::null(),
        flags: Default::default(),
        surface: surface,
        min_image_count: desired_image_count,
        image_color_space: surface_format.color_space,
        image_format: surface_format.format,
        image_extent: surface_resolution.clone(),
        image_array_layers: 1,
        image_usage: vk::IMAGE_USAGE_COLOR_ATTACHMENT_BIT,
        image_sharing_mode: vk::SharingMode::Exclusive,
        queue_family_index_count: 0,
        p_queue_family_indices: ptr::null(),
        pre_transform: surface_capabilities.current_transform,
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

    // Move execution control over to winit, which will call us back for each
    // event.
    //
    // Eventually, we'll want to replace this function with a real loop that
    // tracks timing and peeks the event queue so that we can implement logic!
    events_loop.run_forever(|event| {
        match event {
            winit::Event::WindowEvent { event: winit::WindowEvent::Closed, .. } => {
                winit::ControlFlow::Break
            },
            _ => winit::ControlFlow::Continue,
        }
    });

    // Make sure you clean up after yourself!
    unsafe {
        swapchain_extension.destroy_swapchain_khr(swapchain, None);
        device.destroy_device(None);
        surface_extension.destroy_surface_khr(surface, None);
        debug_report_extension.destroy_debug_report_callback_ext(debug_callback, None);
        instance.destroy_instance(None);
    }
}