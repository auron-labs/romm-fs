use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub enum Action {
    Open,
    Quit,
}
pub struct Tray {
    icon: TrayIcon,
    status: MenuItem,
    open: MenuItem,
    quit: MenuItem,
    last_status: String,
}
impl Tray {
    pub fn new() -> anyhow::Result<Self> {
        let menu = Menu::new();
        let status = MenuItem::new("RomMFS · starting", false, None);
        let open = MenuItem::new("Open RomMFS", true, None);
        let quit = MenuItem::new("Quit", true, None);
        menu.append_items(&[&status, &open, &quit])?;
        let icon = TrayIconBuilder::new()
            .with_tooltip("RomMFS · starting")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_icon(tray_icon::Icon::from_rgba(crate::icon::rgba(32), 32, 32)?)
            .build()?;
        Ok(Self {
            icon,
            status,
            open,
            quit,
            last_status: String::new(),
        })
    }
    pub fn update_status(&mut self, status: String) {
        if self.last_status == status {
            return;
        }
        self.status.set_text(&status);
        let _ = self.icon.set_tooltip(Some(&status));
        self.last_status = status;
    }
    pub fn actions(&self) -> Vec<Action> {
        let mut actions = Vec::new();
        for event in MenuEvent::receiver().try_iter() {
            if event.id == self.open.id() {
                actions.push(Action::Open);
            }
            if event.id == self.quit.id() {
                actions.push(Action::Quit);
            }
        }
        for event in TrayIconEvent::receiver().try_iter() {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                actions.push(Action::Open);
            }
        }
        actions
    }
}

pub fn hide(hwnd: usize) {
    // SAFETY: caller supplies the live GPUI window's native HWND, on its UI thread.
    unsafe {
        ShowWindow(hwnd as _, SW_HIDE);
    }
}
pub fn show(hwnd: usize) {
    // SAFETY: caller supplies the live GPUI window's native HWND, on its UI thread.
    unsafe {
        ShowWindow(hwnd as _, SW_SHOW);
        ShowWindow(hwnd as _, SW_RESTORE);
        SetForegroundWindow(hwnd as _);
    }
}
pub fn set_window_icon(hwnd: usize) {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    // SAFETY: resource id 1 is emitted by winresource. LR_SHARED keeps ownership
    // with the process resource loader; the live window does not free the icons.
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        for (kind, size) in [(ICON_BIG, 32), (ICON_SMALL, 16)] {
            let icon = LoadImageW(module, 1usize as _, IMAGE_ICON, size, size, LR_SHARED);
            if !icon.is_null() {
                SendMessageW(hwnd as _, WM_SETICON, kind as usize, icon as isize);
            }
        }
    }
}
