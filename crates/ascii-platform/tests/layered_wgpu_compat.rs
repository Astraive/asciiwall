#![cfg(windows)]

use std::{
    ffi::c_void,
    mem::size_of,
    ptr::{NonNull, null_mut},
};

use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle,
};
use windows_sys::Win32::{
    Foundation::{GetLastError, HINSTANCE, HWND, SetLastError},
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, GWL_EXSTYLE, GWL_STYLE, HWND_BOTTOM,
        LWA_ALPHA, RegisterClassExW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOOWNERZORDER,
        SetLayeredWindowAttributes, SetParent, SetWindowLongPtrW, SetWindowPos, UnregisterClassW,
        WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_DISABLED, WS_EX_LAYERED,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
    },
};

struct HiddenWindowHierarchy {
    parent: HWND,
    child: HWND,
    instance: HINSTANCE,
}

impl Drop for HiddenWindowHierarchy {
    fn drop(&mut self) {
        // SAFETY: these HWNDs and class registration were created and retained by this test.
        unsafe {
            if !self.child.is_null() {
                DestroyWindow(self.child);
            }
            if !self.parent.is_null() {
                DestroyWindow(self.parent);
            }
            UnregisterClassW(TEST_CLASS_NAME.as_ptr(), self.instance);
        }
    }
}

const TEST_CLASS_NAME: &[u16] = &[
    65, 115, 99, 105, 105, 87, 97, 108, 108, 46, 76, 97, 121, 101, 114, 101, 100, 80, 114, 111, 98,
    101, 0,
];

fn create_hidden_hierarchy() -> HiddenWindowHierarchy {
    let title = [0_u16];
    // SAFETY: null requests the current test module, which remains loaded for this test.
    let instance = unsafe { GetModuleHandleW(null_mut()) };
    assert!(!instance.is_null(), "failed to obtain test module instance");
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        // Match production's registered class: no CS_OWNDC/CS_CLASSDC or other class styles.
        style: 0,
        lpfnWndProc: Some(DefWindowProcW),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: null_mut(),
        hbrBackground: null_mut(),
        lpszMenuName: null_mut(),
        lpszClassName: TEST_CLASS_NAME.as_ptr(),
        hIconSm: null_mut(),
    };
    // SAFETY: class points to initialized storage and its name has static lifetime.
    assert_ne!(
        unsafe { RegisterClassExW(&class) },
        0,
        "failed to register test window class"
    );

    let mut hierarchy = HiddenWindowHierarchy {
        parent: null_mut(),
        child: null_mut(),
        instance,
    };
    // SAFETY: this is an isolated hidden popup with the production-equivalent class and exstyles.
    unsafe { SetLastError(0) };
    hierarchy.parent = unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            TEST_CLASS_NAME.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_DISABLED,
            0,
            0,
            1,
            1,
            null_mut(),
            null_mut(),
            instance,
            null_mut(),
        )
    };
    if hierarchy.parent.is_null() {
        let error = unsafe { GetLastError() };
        panic!("failed to create hidden test parent HWND: Win32 error {error}");
    }

    // Match production's initial hidden surface HWND: a popup, not a child created directly.
    unsafe { SetLastError(0) };
    hierarchy.child = unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            TEST_CLASS_NAME.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_DISABLED | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
            0,
            0,
            1,
            1,
            null_mut(),
            null_mut(),
            instance,
            null_mut(),
        )
    };
    if hierarchy.child.is_null() {
        let error = unsafe { GetLastError() };
        panic!("failed to create hidden popup surface HWND: Win32 error {error}");
    }

    // Follow production's attach sequence: set child and extended styles, then reparent.
    unsafe { SetLastError(0) };
    let old_style = unsafe {
        SetWindowLongPtrW(
            hierarchy.child,
            GWL_STYLE,
            (WS_CHILD | WS_DISABLED | WS_CLIPSIBLINGS | WS_CLIPCHILDREN) as isize,
        )
    };
    let style_error = unsafe { GetLastError() };
    assert!(
        old_style != 0 || style_error == 0,
        "SetWindowLongPtrW(style) failed: Win32 error {style_error}"
    );

    unsafe { SetLastError(0) };
    let old_exstyle = unsafe {
        SetWindowLongPtrW(
            hierarchy.child,
            GWL_EXSTYLE,
            (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_LAYERED) as isize,
        )
    };
    let exstyle_error = unsafe { GetLastError() };
    assert!(
        old_exstyle != 0 || exstyle_error == 0,
        "SetWindowLongPtrW(exstyle) failed: Win32 error {exstyle_error}"
    );

    unsafe { SetLastError(0) };
    let previous_parent = unsafe { SetParent(hierarchy.child, hierarchy.parent) };
    let parent_error = unsafe { GetLastError() };
    assert!(
        !previous_parent.is_null() || parent_error == 0,
        "SetParent failed: Win32 error {parent_error}"
    );

    unsafe { SetLastError(0) };
    let positioned = unsafe {
        SetWindowPos(
            hierarchy.child,
            HWND_BOTTOM,
            0,
            0,
            1,
            1,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        )
    };
    let position_error = unsafe { GetLastError() };
    assert_ne!(
        positioned, 0,
        "SetWindowPos after reparent failed: Win32 error {position_error}"
    );

    // SAFETY: child is a live layered HWND; alpha 255 requests opaque content.
    unsafe { SetLastError(0) };
    let layered = unsafe { SetLayeredWindowAttributes(hierarchy.child, 0, 255, LWA_ALPHA) };
    let layered_error = unsafe { GetLastError() };
    assert_ne!(
        layered, 0,
        "SetLayeredWindowAttributes(alpha=255) failed: Win32 error {layered_error}"
    );
    hierarchy
}

#[test]
#[ignore = "manual GPU/backend compatibility probe; creates only hidden isolated HWNDs"]
fn layered_child_hwnd_accepts_opaque_wgpu_surface() {
    let windows = create_hidden_hierarchy();
    let instance = wgpu::Instance::default();
    let window_handle = NonNull::new(windows.child)
        .expect("test child HWND unexpectedly null")
        .cast::<c_void>();
    let raw_window_handle = RawWindowHandle::Win32(Win32WindowHandle::new(
        std::num::NonZeroIsize::new(window_handle.as_ptr() as isize)
            .expect("test child HWND unexpectedly zero"),
    ));
    let raw_display_handle = RawDisplayHandle::Windows(WindowsDisplayHandle::new());
    // SAFETY: the hidden child remains alive until after the surface is explicitly dropped.
    let surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle,
            raw_window_handle,
        })
    }
    .expect("WGPU failed to create an HWND surface for the layered child");

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: Some(&surface),
    }))
    .expect("no adapter supports the isolated layered HWND surface");
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("layered HWND compatibility probe"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
        },
        None,
    ))
    .expect("failed to create a device for the layered HWND surface");

    let capabilities = surface.get_capabilities(&adapter);
    let format = if capabilities
        .formats
        .contains(&wgpu::TextureFormat::Bgra8UnormSrgb)
    {
        wgpu::TextureFormat::Bgra8UnormSrgb
    } else {
        assert!(
            capabilities
                .formats
                .contains(&wgpu::TextureFormat::Rgba8UnormSrgb)
        );
        wgpu::TextureFormat::Rgba8UnormSrgb
    };
    assert!(
        capabilities
            .present_modes
            .contains(&wgpu::PresentMode::Fifo)
    );
    assert!(
        capabilities
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::Opaque)
    );
    surface.configure(
        &device,
        &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: 1,
            height: 1,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: Vec::new(),
            desired_maximum_frame_latency: 2,
        },
    );

    let frame = surface
        .get_current_texture()
        .expect("failed to acquire a frame for the layered HWND surface");
    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("layered HWND compatibility frame"),
    });
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("layered HWND compatibility clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
    queue.submit(Some(encoder.finish()));
    frame.present();
    device.poll(wgpu::Maintain::Wait);
}
