use crate::config::Result;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{
        Shell::{
            DefSubclassProc, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
            NOTIFYICONDATAW, RemoveWindowSubclass, SetWindowSubclass, Shell_NotifyIconW,
        },
        WindowsAndMessaging::*,
    },
};

const TRAY_MESSAGE: u32 = WM_APP + 1;

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct Placement {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    maximized: bool,
}

struct TrayState {
    sender: windows_reactor::LocalSender<crate::Message>,
    close_to_tray: std::cell::Cell<bool>,
    busy: std::cell::Cell<bool>,
    running: std::cell::Cell<bool>,
    tun: std::cell::Cell<bool>,
    system_proxy: std::cell::Cell<bool>,
}

pub struct Tray {
    data: NOTIFYICONDATAW,
    window_file: Option<std::path::PathBuf>,
    placement: Option<Placement>,
    state: Box<TrayState>,
}

impl Tray {
    pub fn set_busy(&self, busy: bool) {
        self.state.busy.set(busy);
    }
    pub fn exit(&self) -> Result<()> {
        unsafe { exit_window(self.data.hWnd) }
    }
    pub fn hide(&self) {
        unsafe {
            ShowWindow(self.data.hWnd, SW_HIDE);
        }
    }

    pub fn update(
        &mut self,
        directory: &str,
        close_to_tray: bool,
        live: Option<&crate::kernel::LiveState>,
        system_proxy: bool,
    ) -> Result<()> {
        self.state.close_to_tray.set(close_to_tray);
        self.state.running.set(live.is_some());
        self.state.tun.set(live.is_some_and(|live| live.tun));
        self.state.system_proxy.set(system_proxy);
        self.state.busy.set(false);
        let mut tooltip = "mihomo-x · 已停止".to_owned();
        if let Some(live) = live {
            tooltip = format!("mihomo-x · {} · TUN {}", live.mode, live.tun);
        }
        self.data.szTip.fill(0);
        for (destination, character) in self
            .data
            .szTip
            .iter_mut()
            .take(127)
            .zip(tooltip.encode_utf16())
        {
            *destination = character;
        }
        if unsafe { Shell_NotifyIconW(NIM_MODIFY, &self.data) } == 0 {
            return Err("更新托盘状态失败".into());
        }
        if self.window_file.is_some() {
            return Ok(());
        }
        let path = std::path::PathBuf::from(directory).join("window.json");
        if path.exists() {
            let saved: Placement = serde_json::from_slice(&std::fs::read(&path)?)?;
            let width = saved
                .right
                .checked_sub(saved.left)
                .ok_or("保存的窗口宽度无效")?;
            let height = saved
                .bottom
                .checked_sub(saved.top)
                .ok_or("保存的窗口高度无效")?;
            if width <= 0 || height <= 0 || width > 32768 || height > 32768 {
                return Err("保存的窗口尺寸无效".into());
            }
            let mut placement = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            if unsafe { GetWindowPlacement(self.data.hWnd, &mut placement) } == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            placement.rcNormalPosition = windows_sys::Win32::Foundation::RECT {
                left: saved.left,
                top: saved.top,
                right: saved.right,
                bottom: saved.bottom,
            };
            placement.showCmd = SW_SHOWNORMAL as u32;
            if saved.maximized {
                placement.showCmd = SW_SHOWMAXIMIZED as u32;
            }
            if unsafe { SetWindowPlacement(self.data.hWnd, &placement) } == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            self.placement = Some(saved);
        }
        self.window_file = Some(path);
        Ok(())
    }

    pub fn save_window(&mut self) -> Result<()> {
        let Some(path) = &self.window_file else {
            return Ok(());
        };
        let mut placement = WINDOWPLACEMENT {
            length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        if unsafe { GetWindowPlacement(self.data.hWnd, &mut placement) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if placement.showCmd == SW_SHOWMINIMIZED as u32 || placement.showCmd == SW_HIDE as u32 {
            return Ok(());
        }
        let rect = placement.rcNormalPosition;
        let saved = Placement {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
            maximized: placement.showCmd == SW_SHOWMAXIMIZED as u32,
        };
        if self.placement.as_ref() == Some(&saved) {
            return Ok(());
        }
        crate::config::write_atomic(path, &serde_json::to_vec_pretty(&saved)?)?;
        self.placement = Some(saved);
        Ok(())
    }
    pub fn new(sender: windows_reactor::LocalSender<crate::Message>) -> Result<Self> {
        let title: Vec<u16> = "mihomo-x · Mihomo".encode_utf16().chain(Some(0)).collect();
        let window = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
        if window.is_null() {
            return Err("无法找到 mihomo-x 窗口，托盘未初始化".into());
        }
        let module = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null())
        };
        if module.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        // Win32 represents integer resource ID 1 as a pointer-sized value, never dereferenced.
        let icon = unsafe { LoadIconW(module, std::ptr::without_provenance(1)) };
        if icon.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        unsafe {
            SendMessageW(window, WM_SETICON, ICON_SMALL as usize, icon as isize);
            SendMessageW(window, WM_SETICON, ICON_BIG as usize, icon as isize);
        }
        let mut data = NOTIFYICONDATAW {
            cbSize: u32::try_from(std::mem::size_of::<NOTIFYICONDATAW>())?,
            hWnd: window,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: TRAY_MESSAGE,
            hIcon: icon,
            ..Default::default()
        };
        for (destination, character) in data
            .szTip
            .iter_mut()
            .zip("mihomo-x · Mihomo".encode_utf16())
        {
            *destination = character;
        }
        if unsafe { Shell_NotifyIconW(NIM_ADD, &data) } == 0 {
            return Err("创建系统托盘图标失败".into());
        }
        let tray = Self {
            data,
            window_file: None,
            placement: None,
            state: Box::new(TrayState {
                sender,
                close_to_tray: std::cell::Cell::new(true),
                busy: std::cell::Cell::new(false),
                running: std::cell::Cell::new(false),
                tun: std::cell::Cell::new(false),
                system_proxy: std::cell::Cell::new(false),
            }),
        };
        // The boxed state has a stable address and outlives the installed UI-thread callback.
        if unsafe {
            SetWindowSubclass(
                window,
                Some(window_message),
                1,
                std::ptr::from_ref(&*tray.state) as usize,
            )
        } == 0
        {
            return Err("注册关闭到托盘行为失败".into());
        }
        Ok(tray)
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(self.data.hWnd) != 0
                && RemoveWindowSubclass(self.data.hWnd, Some(window_message), 1) == 0
            {
                eprintln!("移除托盘窗口回调失败");
            }
            if Shell_NotifyIconW(NIM_DELETE, &self.data) == 0 {
                eprintln!("移除系统托盘图标失败");
            }
        }
    }
}

// Windows invokes this callback on the UI thread; Tray owns the boxed state until unhooking.
unsafe extern "system" fn window_message(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass: usize,
    data: usize,
) -> LRESULT {
    let state = unsafe { &*(data as *const TrayState) };
    if message == WM_CLOSE {
        if state.close_to_tray.get() {
            unsafe {
                ShowWindow(window, SW_HIDE);
            }
            return 0;
        }
        if !state.sender.send(crate::Message::Exit) {
            eprintln!("窗口退出请求无法发送：消息队列已关闭或已满");
        }
        return 0;
    }
    if message == TRAY_MESSAGE && lparam as u32 == WM_LBUTTONUP {
        unsafe {
            ShowWindow(window, SW_RESTORE);
            SetForegroundWindow(window);
        }
        return 0;
    }
    if message == TRAY_MESSAGE && lparam as u32 == WM_RBUTTONUP {
        let result = unsafe { show_menu(window, state) };
        if let Err(error) = result {
            let text: Vec<u16> = error.to_string().encode_utf16().chain(Some(0)).collect();
            let title: Vec<u16> = "mihomo-x".encode_utf16().chain(Some(0)).collect();
            unsafe { MessageBoxW(window, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR) };
        }
        return 0;
    }
    unsafe { DefSubclassProc(window, message, wparam, lparam) }
}

unsafe fn show_menu(window: HWND, state: &TrayState) -> Result<()> {
    let menu = unsafe { CreatePopupMenu() };
    if menu.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    let result = (|| -> Result<()> {
        let mut connection_label = "启动内核";
        if state.running.get() {
            connection_label = "停止内核";
        }
        for (id, label) in [
            (1, "打开 mihomo-x"),
            (3, connection_label),
            (4, "切换系统代理"),
            (5, "切换 TUN"),
            (6, "规则模式"),
            (7, "全局模式"),
            (8, "直连模式"),
            (2, "退出"),
        ] {
            let text: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
            let mut flags = MF_STRING;
            if id != 1 && state.busy.get() {
                flags |= MF_GRAYED;
            }
            if [4, 5].contains(&id) && !state.running.get() {
                flags |= MF_GRAYED;
            }
            if unsafe { AppendMenuW(menu, flags, id, text.as_ptr()) } == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        let mut point = windows_sys::Win32::Foundation::POINT::default();
        if unsafe { GetCursorPos(&mut point) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        unsafe { SetForegroundWindow(window) };
        let selected = unsafe {
            TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY,
                point.x,
                point.y,
                0,
                window,
                std::ptr::null(),
            )
        };
        if selected == 1 {
            unsafe {
                ShowWindow(window, SW_RESTORE);
                SetForegroundWindow(window);
            }
        }
        let message = match selected {
            0 | 1 => return Ok(()),
            2 => crate::Message::Exit,
            3 => {
                let mut action = crate::backend::Action::Start;
                if state.running.get() {
                    action = crate::backend::Action::Stop;
                }
                crate::Message::Execute(action)
            }
            4 => crate::Message::Execute(crate::backend::Action::SystemProxy(
                !state.system_proxy.get(),
            )),
            5 => crate::Message::Execute(crate::backend::Action::Tun(!state.tun.get())),
            6 => crate::Message::Execute(crate::backend::Action::Mode("rule".into())),
            7 => crate::Message::Execute(crate::backend::Action::Mode("global".into())),
            8 => crate::Message::Execute(crate::backend::Action::Mode("direct".into())),
            _ => return Err("未知托盘操作".into()),
        };
        if !state.sender.send(message) {
            return Err("托盘操作无法发送：窗口已关闭或消息队列已满".into());
        }
        Ok(())
    })();
    if unsafe { DestroyMenu(menu) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    result
}

unsafe fn exit_window(window: HWND) -> Result<()> {
    if unsafe { RemoveWindowSubclass(window, Some(window_message), 1) } == 0 {
        return Err("解除托盘窗口处理失败".into());
    }
    unsafe {
        SendMessageW(window, WM_CLOSE, 0, 0);
    }
    Ok(())
}
