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
use ash::vk::types::{PhysicalDevice};

#[cfg(all(windows))]
fn extension_names() -> Vec<*const i8> {
    vec![
        Surface::name().as_ptr(),
        Win32Surface::name().as_ptr(),
        DebugReport::name().as_ptr(),
    ]
}

#[cfg(windows)]
fn create_surface<E: EntryV1_0, I: InstanceV1_0>(
    entry: &E,
    instance: &I,
    window: &winit::Window,
) -> Result<vk::SurfaceKHR, vk::Result> {
    use winapi::shared::windef::HWND;
    use winapi::um::winuser::GetWindow;
    use winit::os::windows::WindowExt;

    let hwnd = window.get_hwnd() as HWND;
    let hinstance = unsafe { GetWindow(hwnd, 0) as *const vk::c_void };
    let win32_create_info = vk::Win32SurfaceCreateInfoKHR {
        s_type: vk::StructureType::Win32SurfaceCreateInfoKhr,
        p_next: ptr::null(),
        flags: Default::default(),
        hinstance: hinstance,
        hwnd: hwnd as *const vk::c_void,
    };
    let win32_surface_loader = Win32Surface::new(entry, instance).expect("Unable to load win32 surface");
    unsafe { win32_surface_loader.create_win32_surface_khr(&win32_create_info, None) }
}

unsafe extern "system" fn vulkan_debug_callback(
    _: vk::DebugReportFlagsEXT,
    _: vk::DebugReportObjectTypeEXT,
    _: vk::uint64_t,
    _: vk::size_t,
    _: vk::int32_t,
    _: *const vk::c_char,
    p_message: *const vk::c_char,
    _: *mut vk::c_void,
) -> u32 {
    println!("{:?}", CStr::from_ptr(p_message));
    1
}

fn create_instance(entry: &Entry<V1_0>) -> Instance<V1_0> {
    let app_name = CString::new("TryAsh").unwrap();
    let raw_name = app_name.as_ptr();

    let layer_names = [CString::new("VK_LAYER_LUNARG_standard_validation").unwrap()];
    let layers_names_raw: Vec<*const i8> = layer_names
        .iter()
        .map(|layer_name| layer_name.as_ptr())
        .collect();
    let extension_names_raw = extension_names();

    let appinfo = vk::ApplicationInfo {
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
        p_application_info: &appinfo,
        pp_enabled_layer_names: layers_names_raw.as_ptr(),
        enabled_layer_count: layers_names_raw.len() as u32,
        pp_enabled_extension_names: extension_names_raw.as_ptr(),
        enabled_extension_count: extension_names_raw.len() as u32,
    };

    unsafe {
        entry
            .create_instance(&create_info, None)
            .expect("Instance creation error")
    }
}

fn create_debug_callback(entry: &Entry<V1_0>, instance: &Instance<V1_0>) {
    let debug_info = vk::DebugReportCallbackCreateInfoEXT {
        s_type: vk::StructureType::DebugReportCallbackCreateInfoExt,
        p_next: ptr::null(),
        flags: vk::DEBUG_REPORT_ERROR_BIT_EXT | vk::DEBUG_REPORT_WARNING_BIT_EXT
            | vk::DEBUG_REPORT_PERFORMANCE_WARNING_BIT_EXT,
        pfn_callback: vulkan_debug_callback,
        p_user_data: ptr::null_mut(),
    };
    let debug_report_loader = DebugReport::new(entry, instance).expect("Unable to load debug report");
    unsafe {
        debug_report_loader
            .create_debug_report_callback_ext(&debug_info, None)
            .unwrap();
    };
}

fn main() {
    let (window_width, window_height) = (800, 600);

    let events_loop = winit::EventsLoop::new();
    let window = winit::WindowBuilder::new()
        .with_title("Try Ash")
        .with_dimensions(window_width, window_height)
        .build(&events_loop)
        .unwrap();

    let entry = Entry::<V1_0>::new().unwrap();
    let instance = create_instance(&entry);
    create_debug_callback(&entry, &instance);

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

    loop {}
}
