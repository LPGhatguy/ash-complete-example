use std::default::Default;
use std::ffi::{CStr, CString};
use std::ptr;

use winit;

use ash::{Entry, Instance, Device, vk};
use ash::version::{DeviceV1_0, EntryV1_0, InstanceV1_0, V1_0};
use ash::extensions;

// Applications using Vulkan have to give Vulkan a name
const APP_NAME: *const i8 = cstr!("The Game");
const ENGINE_NAME: *const i8 = cstr!("Two-Stroke");

pub struct VulkanContext {
    pub window: winit::Window,
    pub events_loop: winit::EventsLoop,

    pub entry: Entry<V1_0>,
    pub instance: Instance<V1_0>,
    pub device: Device<V1_0>,

    pub surface: vk::SurfaceKHR,
    pub the_queue: u32,
    pub physical_device: vk::PhysicalDevice,
    pub debug_callback: vk::DebugReportCallbackEXT,

    pub debug_extension: extensions::DebugReport,
    pub surface_extension: extensions::Surface,
    pub swapchain_extension: extensions::Swapchain,
}

impl VulkanContext {
    pub fn new() -> VulkanContext {
        let (window_width, window_height) = (800, 600);

        let (window, events_loop) = create_window(window_width, window_height);
        let entry = create_vulkan_entry();
        let instance = create_vulkan_instance(&entry);

        // Load VK_EXT_debug_report extension
        let debug_extension = extensions::DebugReport::new(&entry, &instance)
            .expect("Unable to load DebugReport extension");

        let debug_callback = set_up_debug_callback(&debug_extension);

        // Load VK_KHR_surface extension
        let surface_extension = extensions::Surface::new(&entry, &instance)
            .expect("Unable to load the Surface extension");

        let surface = create_surface(&entry, &instance, &window)
            .expect("Failed to create surface!");

        let (physical_device, the_queue) = choose_physical_device_and_queue_family(
            &instance,
            &surface_extension,
            surface
        );

        let device = create_logical_device(
            &instance,
            physical_device,
            the_queue,
        );

        // Load VK_KHR_swapchain extension
        let swapchain_extension = extensions::Swapchain::new(&instance, &device)
            .expect("Unable to load Swapchain extension!");

        VulkanContext {
            window,
            events_loop,

            entry,
            instance,
            device,

            surface,
            the_queue,
            physical_device,
            debug_callback,

            debug_extension,
            surface_extension,
            swapchain_extension,
        }
    }
}

impl Drop for VulkanContext {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_device(None);
            self.surface_extension.destroy_surface_khr(self.surface, None);
            self.debug_extension.destroy_debug_report_callback_ext(self.debug_callback, None);
            self.instance.destroy_instance(None);
        }
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

fn create_window(width: u32, height: u32) -> (winit::Window, winit::EventsLoop) {
    // Construct a regular winit events loop and window; nothing special here.
    let events_loop = winit::EventsLoop::new();
    let window = winit::WindowBuilder::new()
        .with_title("Ash Triangle")
        .with_dimensions(width, height)
        .build(&events_loop)
        .expect("Unable to construct winit window!");

    (window, events_loop)
}

fn create_vulkan_entry() -> Entry<V1_0> {
    // 'Entry' implements a specific API version and automatically loads
    // function pointers for us.
    Entry::<V1_0>::new()
        .expect("Unable to create Vulkan Entry!")
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
        p_engine_name: ENGINE_NAME,
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
fn choose_physical_device_and_queue_family(
    instance: &Instance<V1_0>,
    surface_extension: &extensions::Surface,
    surface: vk::SurfaceKHR,
) -> (vk::PhysicalDevice, u32)
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

fn set_up_debug_callback(debug_extension: &extensions::DebugReport) -> vk::DebugReportCallbackEXT {
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
        debug_extension
            .create_debug_report_callback_ext(&debug_info, None)
            .expect("Unable to attach DebugReport callback!")
    };

    debug_callback
}