//! Minimal OpenVR client: just enough of IVROverlay/IVRSystem to show one dashboard overlay.
//! openvr_api is loaded at runtime so the binary starts (and just waits) without SteamVR.

use super::openvr_tables::{OverlayFnTable, SystemFnTable};
use anyhow::{Result, anyhow, bail};
use libloading::{Library, Symbol};
use std::{
    ffi::{CString, c_char},
    path::PathBuf,
};

const OVERLAY_INTERFACE: &str = "FnTable:IVROverlay_028";
const SYSTEM_INTERFACE: &str = "FnTable:IVRSystem_026";
// Background apps never start SteamVR (and do not keep it alive): this app polls until SteamVR is
// running, an Overlay app would launch it on its own.
const APPLICATION_TYPE_BACKGROUND: i32 = 3;
const INPUT_METHOD_MOUSE: i32 = 1;
const FLAG_SEND_DISCRETE_SCROLL: i32 = 64;

const EVENT_MOUSE_MOVE: u32 = 300;
const EVENT_MOUSE_DOWN: u32 = 301;
const EVENT_MOUSE_UP: u32 = 302;
const EVENT_FOCUS_LEAVE: u32 = 304;
const EVENT_SCROLL_DISCRETE: u32 = 305;
const EVENT_QUIT: u32 = 700;

pub const MOUSE_BUTTON_LEFT: u32 = 1;

// Mirrors VREvent_t on 64 bit: 12 bytes of header, 4 of padding (the data union is 8 aligned),
// then the 48 bytes union.
#[repr(C, align(8))]
#[allow(dead_code)]
pub struct VrEvent {
    event_type: u32,
    tracked_device_index: u32,
    event_age_seconds: f32,
    padding: u32,
    data: [u8; 48],
}

const _: () = assert!(size_of::<VrEvent>() == 64);

impl VrEvent {
    fn new() -> Self {
        Self {
            event_type: 0,
            tracked_device_index: 0,
            event_age_seconds: 0.0,
            padding: 0,
            data: [0; 48],
        }
    }

    fn f32_at(&self, offset: usize) -> f32 {
        f32::from_ne_bytes(self.data[offset..offset + 4].try_into().unwrap())
    }

    fn u32_at(&self, offset: usize) -> u32 {
        u32::from_ne_bytes(self.data[offset..offset + 4].try_into().unwrap())
    }
}

pub enum OverlayEvent {
    // Position in texture pixels, origin top-left
    MouseMove { x: f32, y: f32 },
    MouseButton { x: f32, y: f32, button: u32, pressed: bool },
    Scroll { x: f32, y: f32 },
    FocusLeave,
    // SteamVR is closing
    Quit,
}

type InitFn = unsafe extern "C" fn(*mut i32, i32) -> isize;
type ShutdownFn = unsafe extern "C" fn();
type GetInterfaceFn = unsafe extern "C" fn(*const c_char, *mut i32) -> isize;

pub struct OpenVr {
    // Keeps the library mapped for as long as the tables are used
    _lib: Library,
    shutdown: ShutdownFn,
    overlay: &'static OverlayFnTable,
    system: &'static SystemFnTable,
    handle: u64,
    thumbnail_handle: u64,
    texture_size: [u32; 2],
}

fn library_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![];

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join(LIB_NAME));
    }

    // Development checkout
    candidates.push(PathBuf::from("openvr/bin/win64/openvr_api.dll"));

    // SteamVR itself ships a copy
    if cfg!(windows) {
        candidates.push(PathBuf::from(
            r"C:\Program Files (x86)\Steam\steamapps\common\SteamVR\bin\win64\openvr_api.dll",
        ));
    }

    candidates.push(PathBuf::from(LIB_NAME));

    candidates
}

#[cfg(windows)]
const LIB_NAME: &str = "openvr_api.dll";
#[cfg(not(windows))]
const LIB_NAME: &str = "libopenvr_api.so";

unsafe fn interface<T>(get: &Symbol<GetInterfaceFn>, name: &str) -> Result<&'static T> {
    let name = CString::new(name)?;
    let mut error = 0;
    let ptr = unsafe { get(name.as_ptr(), &mut error) };
    if ptr == 0 || error != 0 {
        bail!("VR_GetGenericInterface({name:?}) failed with error {error}");
    }

    Ok(unsafe { &*(ptr as *const T) })
}

impl OpenVr {
    /// Fails while SteamVR is not running: the caller is expected to retry.
    pub fn connect(texture_size: [u32; 2], width_meters: f32) -> Result<Self> {
        let lib = library_candidates()
            .into_iter()
            .find_map(|path| unsafe { Library::new(path).ok() })
            .ok_or_else(|| anyhow!("{LIB_NAME} not found"))?;

        unsafe {
            let init: Symbol<InitFn> = lib.get(b"VR_InitInternal")?;
            let shutdown: Symbol<ShutdownFn> = lib.get(b"VR_ShutdownInternal")?;
            let shutdown = *shutdown;
            let get_interface: Symbol<GetInterfaceFn> = lib.get(b"VR_GetGenericInterface")?;

            let mut error = 0;
            init(&mut error, APPLICATION_TYPE_BACKGROUND);
            if error != 0 {
                bail!("VR_InitInternal failed with error {error}");
            }

            let result = (|| {
                let overlay: &'static OverlayFnTable = interface(&get_interface, OVERLAY_INTERFACE)?;
                let system: &'static SystemFnTable = interface(&get_interface, SYSTEM_INTERFACE)?;

                let key = CString::new("alvr.overlay.settings")?;
                let name = CString::new("ALVR")?;
                let mut handle = 0;
                let mut thumbnail_handle = 0;
                let error = (overlay.CreateDashboardOverlay)(
                    key.as_ptr(),
                    name.as_ptr(),
                    &mut handle,
                    &mut thumbnail_handle,
                );
                if error != 0 {
                    bail!("CreateDashboardOverlay failed with error {error}");
                }

                (overlay.SetOverlayWidthInMeters)(handle, width_meters);
                (overlay.SetOverlayInputMethod)(handle, INPUT_METHOD_MOUSE);
                (overlay.SetOverlayFlag)(handle, FLAG_SEND_DISCRETE_SCROLL, true);
                let mouse_scale = [texture_size[0] as f32, texture_size[1] as f32];
                (overlay.SetOverlayMouseScale)(handle, &mouse_scale);

                Ok((overlay, system, handle, thumbnail_handle))
            })();

            match result {
                Ok((overlay, system, handle, thumbnail_handle)) => Ok(Self {
                    _lib: lib,
                    shutdown,
                    overlay,
                    system,
                    handle,
                    thumbnail_handle,
                    texture_size,
                }),
                Err(e) => {
                    shutdown();
                    Err(e)
                }
            }
        }
    }

    pub fn is_dashboard_visible(&self) -> bool {
        unsafe { (self.overlay.IsDashboardVisible)() }
    }

    /// `rgba` is a tightly packed RGBA8 image with the size given to `connect`.
    pub fn set_texture(&self, rgba: &[u8]) {
        debug_assert_eq!(
            rgba.len(),
            (self.texture_size[0] * self.texture_size[1] * 4) as usize
        );
        unsafe {
            (self.overlay.SetOverlayRaw)(
                self.handle,
                rgba.as_ptr(),
                self.texture_size[0],
                self.texture_size[1],
                4,
            );
        }
    }

    pub fn set_thumbnail(&self, rgba: &[u8], size: [u32; 2]) {
        unsafe {
            (self.overlay.SetOverlayRaw)(self.thumbnail_handle, rgba.as_ptr(), size[0], size[1], 4);
        }
    }

    pub fn poll_events(&self) -> Vec<OverlayEvent> {
        let mut events = vec![];
        let height = self.texture_size[1] as f32;
        let size = size_of::<VrEvent>() as u32;

        let mut event = VrEvent::new();
        while unsafe { (self.overlay.PollNextOverlayEvent)(self.handle, &mut event, size) } {
            // Mouse data: x, y (GL space, origin bottom-left), button
            let (x, y) = (event.f32_at(0), height - event.f32_at(4));

            match event.event_type {
                EVENT_MOUSE_MOVE => events.push(OverlayEvent::MouseMove { x, y }),
                EVENT_MOUSE_DOWN | EVENT_MOUSE_UP => events.push(OverlayEvent::MouseButton {
                    x,
                    y,
                    button: event.u32_at(8),
                    pressed: event.event_type == EVENT_MOUSE_DOWN,
                }),
                EVENT_SCROLL_DISCRETE => events.push(OverlayEvent::Scroll {
                    x: event.f32_at(0),
                    y: event.f32_at(4),
                }),
                EVENT_FOCUS_LEAVE => events.push(OverlayEvent::FocusLeave),
                _ => (),
            }
        }

        // SteamVR waits for the quit acknowledgement before closing
        let mut event = VrEvent::new();
        while unsafe { (self.system.PollNextEvent)(&mut event, size) } {
            if event.event_type == EVENT_QUIT {
                unsafe { (self.system.AcknowledgeQuit_Exiting)() };
                events.push(OverlayEvent::Quit);
            }
        }

        events
    }
}

impl Drop for OpenVr {
    fn drop(&mut self) {
        unsafe {
            (self.overlay.DestroyOverlay)(self.handle);
            (self.shutdown)();
        }
    }
}
