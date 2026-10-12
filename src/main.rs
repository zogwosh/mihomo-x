#![windows_subsystem = "windows"]

mod app_update;
mod backend;
mod config;
mod kernel;
mod startup;
mod system_proxy;
mod tray;

use backend::{Action, Backend, Snapshot};
use std::sync::{Arc, Mutex};
use windows_reactor::*;

#[derive(Clone)]
struct Input(Arc<Mutex<Backend>>, Arc<kernel::DownloadProgress>);

impl PartialEq for Input {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Overview,
    Profiles,
    Nodes,
    Settings,
    Logs,
}

enum Message {
    Browse,
    Path(String),
    Merge(String),
    Search(String),
    LogSearch(String),
    LogLevel(String),
    SortDelays(bool),
    ProfileName(uuid::Uuid, String),
    RemoveRequested(uuid::Uuid),
    CancelRemove,
    Exit,
    Navigate(Option<String>),
    DismissStatus,
    PaneOpen(bool),
    NavigationDisplay(NavigationViewDisplayMode),
    AdvancedExpanded(bool),
    RenameExpanded(uuid::Uuid, bool),
    Execute(Action),
    Finished {
        snapshot: Option<Box<Snapshot>>,
        result: std::result::Result<String, String>,
    },
    Tick,
}

struct ClientApp {
    tray: Option<tray::Tray>,
    backend: Arc<Mutex<Backend>>,
    snapshot: Option<Snapshot>,
    path: String,
    merge: String,
    search: String,
    log_search: String,
    log_level: String,
    sort_delays: bool,
    profile_names: std::collections::BTreeMap<uuid::Uuid, String>,
    pending_remove: Option<uuid::Uuid>,
    status: String,
    status_error: bool,
    status_visible: bool,
    busy: bool,
    page: Page,
    pane_open: bool,
    compact_navigation: bool,
    advanced_expanded: bool,
    rename_expanded: std::collections::BTreeMap<uuid::Uuid, bool>,
    progress: Arc<kernel::DownloadProgress>,
    operation: Option<&'static str>,
    pending_exit: bool,
    pending_actions: std::collections::VecDeque<Action>,
}

impl ClientApp {
    fn exit(&mut self) {
        if let Some(tray) = self.tray.as_mut() {
            let result = tray.save_window().and_then(|()| tray.exit());
            if let Err(error) = result {
                self.status = format!("退出失败：{error}");
                self.status_error = true;
                self.status_visible = true;
            }
        }
    }
    fn execute(&mut self, action: Action, context: &ComponentContext<Self>) {
        if self.busy {
            if !matches!(action, Action::Refresh) {
                self.pending_actions.push_back(action);
                self.status = "操作已排队，正在等待前一个操作完成…".into();
            }
            return;
        }
        self.busy = true;
        if let Some(tray) = &self.tray {
            tray.set_busy(!matches!(action, Action::Refresh));
        }
        self.operation = match &action {
            Action::Refresh => None,
            Action::Upgrade => Some("正在升级内核"),
            Action::DelayGroup(_) => Some("正在批量测速"),
            Action::CheckUpdate => Some("正在检查内核更新"),
            Action::Import(_) | Action::UpdateProfile(_) => Some("正在读取和验证配置"),
            _ => Some("正在应用操作"),
        };
        self.progress
            .stage
            .store(0, std::sync::atomic::Ordering::Relaxed);
        let backend = self.backend.clone();
        context.spawn_background(move |_| {
            let mut backend = match backend.lock() {
                Ok(backend) => backend,
                Err(error) => {
                    return Message::Finished {
                        snapshot: None,
                        result: Err(format!("后台状态锁失效: {error}")),
                    };
                }
            };
            let result = backend.apply(action).map_err(|error| error.to_string());
            match backend.snapshot() {
                Ok(snapshot) => Message::Finished {
                    snapshot: Some(Box::new(snapshot)),
                    result,
                },
                Err(error) => {
                    let detail = match result {
                        Ok(_) => error.to_string(),
                        Err(action_error) => format!("{action_error}；状态读取失败: {error}"),
                    };
                    Message::Finished {
                        snapshot: None,
                        result: Err(detail),
                    }
                }
            }
        });
    }

    fn tick(context: &ComponentContext<Self>) {
        context.spawn_background(|_| {
            std::thread::sleep(std::time::Duration::from_secs(3));
            Message::Tick
        });
    }
}

impl Component for ClientApp {
    type Input = Input;
    type Message = Message;

    fn create(input: &Input, context: &ComponentContext<Self>) -> Self {
        let mut app = Self {
            tray: None,
            backend: input.0.clone(),
            snapshot: None,
            path: String::new(),
            merge: String::new(),
            search: String::new(),
            log_search: String::new(),
            log_level: "all".into(),
            sort_delays: false,
            profile_names: std::collections::BTreeMap::new(),
            pending_remove: None,
            status: "正在加载…".into(),
            status_error: false,
            status_visible: true,
            busy: false,
            page: Page::Overview,
            pane_open: true,
            compact_navigation: false,
            advanced_expanded: false,
            rename_expanded: std::collections::BTreeMap::new(),
            progress: input.1.clone(),
            operation: None,
            pending_exit: false,
            pending_actions: std::collections::VecDeque::new(),
        };
        app.execute(Action::Initialize, context);
        Self::tick(context);
        app
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::Browse => {
                use std::{ffi::OsString, os::windows::ffi::OsStringExt};
                use windows_sys::Win32::{
                    UI::Controls::Dialogs::{
                        CommDlgExtendedError, GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST,
                        OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
                    },
                    UI::Input::KeyboardAndMouse::GetActiveWindow,
                };
                let mut buffer = vec![0_u16; 32768];
                let filter: Vec<u16> = "YAML 配置\0*.yaml;*.yml\0所有文件\0*.*\0\0"
                    .encode_utf16()
                    .collect();
                let title: Vec<u16> = "导入 profile".encode_utf16().chain(Some(0)).collect();
                // The Win32 dialog borrows these buffers only until the modal call returns.
                let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
                dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
                dialog.hwndOwner = unsafe { GetActiveWindow() };
                dialog.lpstrFilter = filter.as_ptr();
                dialog.lpstrFile = buffer.as_mut_ptr();
                dialog.nMaxFile = buffer.len() as u32;
                dialog.lpstrTitle = title.as_ptr();
                dialog.Flags =
                    OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
                if unsafe { GetOpenFileNameW(&mut dialog) } != 0 {
                    let length = buffer
                        .iter()
                        .position(|character| *character == 0)
                        .expect("file dialog must terminate its path");
                    self.path = OsString::from_wide(&buffer[..length])
                        .to_string_lossy()
                        .into_owned();
                    return;
                }
                let error = unsafe { CommDlgExtendedError() };
                if error != 0 {
                    self.status = format!("文件选择失败: {error:#x}");
                    self.status_error = true;
                    self.status_visible = true;
                }
            }
            Message::Path(path) => self.path = path,
            Message::Merge(merge) => self.merge = merge,
            Message::Search(search) => self.search = search,
            Message::LogSearch(search) => self.log_search = search,
            Message::LogLevel(level) => self.log_level = level,
            Message::SortDelays(enabled) => self.sort_delays = enabled,
            Message::PaneOpen(open) => self.pane_open = open,
            Message::NavigationDisplay(mode) => {
                self.compact_navigation = mode != NavigationViewDisplayMode::Expanded;
                self.pane_open = !self.compact_navigation;
            }
            Message::AdvancedExpanded(expanded) => self.advanced_expanded = expanded,
            Message::RenameExpanded(id, expanded) => {
                self.rename_expanded.insert(id, expanded);
            }
            Message::Navigate(tag) => {
                if self.compact_navigation {
                    self.pane_open = false;
                }
                match tag.as_deref() {
                    Some("overview") => self.page = Page::Overview,
                    Some("profiles") => self.page = Page::Profiles,
                    Some("nodes") => self.page = Page::Nodes,
                    Some("settings") => self.page = Page::Settings,
                    Some("logs") => self.page = Page::Logs,
                    _ => {}
                }
            }
            Message::DismissStatus => self.status_visible = false,
            Message::ProfileName(id, name) => {
                self.profile_names.insert(id, name);
            }
            Message::RemoveRequested(id) => self.pending_remove = Some(id),
            Message::CancelRemove => self.pending_remove = None,
            Message::Exit => {
                if self.busy {
                    self.pending_exit = true;
                    self.pending_actions.clear();
                    self.status = "当前操作完成后退出…".into();
                    return;
                }
                self.exit();
            }
            Message::Execute(action) => {
                if matches!(action, Action::Remove(_)) {
                    self.pending_remove = None;
                }
                self.execute(action, context);
            }
            Message::Finished { snapshot, result } => {
                self.busy = false;
                if let Some(tray) = &self.tray {
                    tray.set_busy(false);
                }
                self.operation = None;
                let succeeded = result.is_ok();
                let first_snapshot = self.snapshot.is_none();
                if self.snapshot.is_none()
                    && let Some(snapshot) = &snapshot
                {
                    self.merge = snapshot.settings.merge_yaml.clone();
                    if snapshot.settings.selected.is_none() {
                        self.page = Page::Profiles;
                    }
                }
                if let Some(snapshot) = snapshot {
                    self.snapshot = Some(*snapshot);
                }
                match result {
                    Ok(message) if !message.is_empty() => {
                        self.status = message;
                        self.status_error = false;
                        self.status_visible = true;
                    }
                    Ok(_) => {
                        if self.status == "正在加载…" {
                            self.status = "就绪".into();
                        }
                    }
                    Err(error) => {
                        self.status = format!("错误：{error}");
                        self.status_error = true;
                        self.status_visible = true;
                    }
                }
                if self.tray.is_none() {
                    match tray::Tray::new(context.sender()) {
                        Ok(tray) => self.tray = Some(tray),
                        Err(error) => {
                            self.status = format!("{}；托盘初始化失败：{error}", self.status);
                            self.status_error = true;
                            self.status_visible = true;
                        }
                    }
                }
                if let (Some(tray), Some(snapshot)) = (self.tray.as_mut(), &self.snapshot) {
                    let result = tray.update(
                        &snapshot.directory,
                        snapshot.settings.close_to_tray,
                        snapshot.live.as_ref(),
                        snapshot.settings.system_proxy,
                    );
                    let desktop_updated = result.is_ok();
                    if let Err(error) = result {
                        self.status = format!("桌面状态更新失败：{error}");
                        self.status_error = true;
                        self.status_visible = true;
                    }
                    if first_snapshot
                        && succeeded
                        && desktop_updated
                        && snapshot.settings.start_hidden
                    {
                        tray.hide();
                    }
                }
                if first_snapshot
                    && self.snapshot.is_some()
                    && self.tray.is_some()
                    && let Some(path) = std::env::var_os("MIHOMO_X_UPDATE_READY")
                    && let Err(error) = config::write_atomic(
                        std::path::Path::new(&path),
                        env!("CARGO_PKG_VERSION").as_bytes(),
                    )
                {
                    self.status = format!("更新启动确认失败：{error}");
                    self.status_error = true;
                    self.status_visible = true;
                }
                if succeeded
                    && self
                        .backend
                        .lock()
                        .is_ok_and(|backend| backend.restart_for_update)
                {
                    self.pending_actions.clear();
                    self.pending_exit = true;
                }
                if self.pending_exit {
                    self.pending_exit = false;
                    self.exit();
                } else if first_snapshot && succeeded {
                    self.execute(Action::CheckAppUpdate, context);
                } else if let Some(action) = self.pending_actions.pop_front() {
                    self.execute(action, context);
                }
            }
            Message::Tick => {
                if let Some(tray) = self.tray.as_mut()
                    && let Err(error) = tray.save_window()
                {
                    self.status = format!("窗口位置保存失败：{error}");
                }
                if !self.busy
                    && self.snapshot.as_ref().is_some_and(|snapshot| {
                        snapshot.live.is_some()
                            || snapshot.settings.subscription_interval_seconds > 0
                            || self.page == Page::Logs
                    })
                {
                    self.execute(Action::Refresh, context);
                }
                Self::tick(context);
            }
        }
    }

    fn view(&self, _input: &Input, context: &mut ViewContext<Self>) -> View {
        context.window_title("mihomo-x · Mihomo");
        let available = !self.busy || self.operation.is_none();
        context.window_visuals(
            WindowVisuals::new()
                .backdrop(WindowBackdrop::Mica)
                .client_size(1040.0, 760.0)
                .constraints(WindowConstraints {
                    min_width: Some(560.0),
                    min_height: Some(480.0),
                    ..WindowConstraints::default()
                }),
        );
        let (title, subtitle) = match self.page {
            Page::Overview => ("概览", "管理连接与流量接管方式"),
            Page::Profiles => ("配置", "导入订阅或本地配置，管理你的连接来源"),
            Page::Nodes => ("节点", "选择策略组节点，查看连接延迟"),
            Page::Settings => ("设置", "桌面行为、内核更新与诊断"),
            Page::Logs => ("日志", "查看内核日志，按级别和关键词筛选"),
        };
        let mut rows: Vec<View> = vec![
            StackPanel::new().spacing(4.0).children((
                TextBlock::new()
                    .text(title)
                    .font_size(28.0)
                    .font_weight(FontWeight::SEMI_BOLD),
                TextBlock::new()
                    .text(subtitle)
                    .text_wrapping(TextWrapping::Wrap),
            )),
        ];
        if let Some(operation) = self.operation {
            rows.push(TextBlock::new().text(operation).into());
            use std::sync::atomic::Ordering;
            let stage = self.progress.stage.load(Ordering::Relaxed);
            let downloaded = self.progress.downloaded.load(Ordering::Relaxed);
            let total = self.progress.total.load(Ordering::Relaxed);
            if stage == 1 && total > 0 {
                rows.push(
                    ProgressBar::new()
                        .automation_id("DownloadProgress")
                        .minimum(0.0)
                        .maximum(total as f64)
                        .value(downloaded as f64)
                        .into(),
                );
                rows.push(
                    TextBlock::new()
                        .text(format!(
                            "已下载 {} / {} KiB",
                            downloaded / 1024,
                            total / 1024
                        ))
                        .into(),
                );
            } else {
                rows.push(
                    ProgressBar::new()
                        .automation_id("OperationProgress")
                        .is_indeterminate(true)
                        .into(),
                );
            }
            if stage == 2 {
                rows.push(TextBlock::new().text("正在校验、替换并恢复连接…").into());
            }
            if stage == 3 {
                rows.push(
                    TextBlock::new()
                        .automation_id("BatchDelayProgressText")
                        .text(format!("已测速 {downloaded} / {total}"))
                        .into(),
                );
            }
        }
        let Some(snapshot) = &self.snapshot else {
            rows.push(
                TextBlock::new()
                    .text(&self.status)
                    .text_wrapping(TextWrapping::Wrap)
                    .into(),
            );
            return StackPanel::new().spacing(12.0).margin(24.0).keyed_children(
                rows.into_iter()
                    .enumerate()
                    .map(|(index, row)| (index.to_string(), row)),
            );
        };
        let running = snapshot.live.is_some();
        if self.page == Page::Overview {
            let connection_start = rows.len();
            if let Some(profile) = snapshot
                .settings
                .profiles
                .iter()
                .find(|profile| snapshot.settings.selected == Some(profile.id))
            {
                rows.push(
                    TextBlock::new()
                        .automation_id("CurrentProfileText")
                        .text(format!("当前配置：{}", profile.name))
                        .text_wrapping(TextWrapping::Wrap)
                        .into(),
                );
            }
            let mut mode = snapshot.settings.mode.as_deref();
            let mut tun = snapshot.settings.tun;
            if let Some(live) = &snapshot.live {
                mode = Some(&live.mode);
                tun = Some(live.tun);
            }
            let mut kernel_status = format!("内核 {} · 已停止", snapshot.core_version);
            if let Some(live) = &snapshot.live {
                let mode_name = match live.mode.as_str() {
                    "global" => "全局",
                    "rule" => "规则",
                    "direct" => "直连",
                    value => value,
                };
                let mut tun_state = "关闭";
                if live.tun {
                    tun_state = "开启";
                }
                kernel_status = format!(
                    "内核 {} · 运行中 · {} · TUN {}",
                    live.version, mode_name, tun_state
                );
            }
            rows.push(
                TextBlock::new()
                    .automation_id("KernelStatusText")
                    .text(kernel_status)
                    .into(),
            );
            rows.push(
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .children((
                        Button::new()
                            .automation_id("StartButton")
                            .style(ButtonStyle::Accent)
                            .is_enabled(
                                available && !running && snapshot.settings.selected.is_some(),
                            )
                            .on_click(context.callback(|()| Message::Execute(Action::Start)))
                            .content("启动内核"),
                        Button::new()
                            .automation_id("StopButton")
                            .is_enabled(available && running)
                            .on_click(context.callback(|()| Message::Execute(Action::Stop)))
                            .content("停止内核"),
                        Button::new()
                            .automation_id("RefreshButton")
                            .is_enabled(available)
                            .on_click(context.callback(|()| Message::Execute(Action::Refresh)))
                            .content("刷新"),
                    )),
            );
            let connection = rows.split_off(connection_start);
            rows.push(card(connection));
            if let Some(live) = &snapshot.live {
                let mut rates = "正在采样流量…".to_owned();
                if let (Some(upload), Some(download)) = (live.upload_rate, live.download_rate) {
                    rates = format!(
                        "上传 {:.1} KiB/s · 下载 {:.1} KiB/s",
                        upload / 1024.0,
                        download / 1024.0
                    );
                }
                rows.push(card(vec![
                    TextBlock::new()
                        .automation_id("TrafficRateText")
                        .text(rates)
                        .into(),
                    TextBlock::new()
                        .automation_id("TrafficTotalText")
                        .text(format!(
                            "本次内核运行：上传 {:.2} MiB · 下载 {:.2} MiB · 活动连接 {}",
                            live.upload_total as f64 / 1048576.0,
                            live.download_total as f64 / 1048576.0,
                            live.connection_count
                        ))
                        .into(),
                ]));
            }
            rows.push(section(
                "路由模式",
                "规则按配置分流；全局使用 GLOBAL 策略组；直连跳过代理。",
            ));
            let mode_start = rows.len();
            rows.push(
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .children((
                        Button::new()
                            .automation_id("RuleButton")
                            .style(if mode == Some("rule") {
                                ButtonStyle::Accent
                            } else {
                                ButtonStyle::Default
                            })
                            .is_enabled(available)
                            .on_click(
                                context
                                    .callback(|()| Message::Execute(Action::Mode("rule".into()))),
                            )
                            .content("规则"),
                        Button::new()
                            .automation_id("GlobalButton")
                            .style(if mode == Some("global") {
                                ButtonStyle::Accent
                            } else {
                                ButtonStyle::Default
                            })
                            .is_enabled(available)
                            .on_click(
                                context
                                    .callback(|()| Message::Execute(Action::Mode("global".into()))),
                            )
                            .content("全局"),
                        Button::new()
                            .automation_id("DirectButton")
                            .style(if mode == Some("direct") {
                                ButtonStyle::Accent
                            } else {
                                ButtonStyle::Default
                            })
                            .is_enabled(available)
                            .on_click(
                                context
                                    .callback(|()| Message::Execute(Action::Mode("direct".into()))),
                            )
                            .content("直连"),
                    )),
            );
            let modes = rows.split_off(mode_start);
            rows.push(card(modes));
            rows.push(section(
                "流量接管",
                "系统代理与 TUN 可以独立开启。停止连接时会恢复系统代理。",
            ));
            let mut proxy_description = snapshot.system_proxy.as_str();
            if !running && snapshot.settings.system_proxy {
                proxy_description = "连接启动后自动开启";
            }
            rows.push(setting_row(
                "Windows 系统代理",
                proxy_description,
                ToggleSwitch::new()
                    .automation_id("SystemProxySwitch")
                    .is_on(snapshot.settings.system_proxy)
                    .is_enabled(available && (running || snapshot.settings.system_proxy))
                    .on_toggled(
                        context.callback(|enabled| Message::Execute(Action::SystemProxy(enabled))),
                    ),
            ));
            let tun_description = if running {
                "通过虚拟网卡接管流量"
            } else {
                "启动连接时应用此设置"
            };
            rows.push(setting_row(
                "TUN 模式",
                tun_description,
                ToggleSwitch::new()
                    .automation_id("TunSwitch")
                    .is_on(tun == Some(true))
                    .is_enabled(available)
                    .on_toggled(context.callback(|enabled| Message::Execute(Action::Tun(enabled)))),
            ));
        }
        if self.page == Page::Profiles {
            rows.push(section(
                "添加配置",
                "支持 YAML 文件和 HTTP / HTTPS 订阅地址。",
            ));
            let import_start = rows.len();
            if snapshot.settings.profiles.is_empty() {
                rows.push(
                    TextBlock::new()
                        .text("尚未导入配置。选择本地 YAML 文件，或输入订阅 URL。")
                        .into(),
                );
            }
            rows.push(
                Button::new()
                    .automation_id("BrowseButton")
                    .is_enabled(available)
                    .on_click(context.callback(|()| Message::Browse))
                    .content("选择配置文件…"),
            );
            rows.push(
                TextBox::new()
                    .automation_id("ProfilePathBox")
                    .text(&self.path)
                    .placeholder_text("本地 YAML 路径或订阅 URL (http / https)")
                    .is_enabled(available)
                    .on_text_changed(context.callback(Message::Path))
                    .into(),
            );
            let import_path = self.path.clone();
            rows.push(
                Button::new()
                    .automation_id("ImportButton")
                    .is_enabled(available && !self.path.trim().is_empty())
                    .on_click(
                        context.callback(move |()| {
                            Message::Execute(Action::Import(import_path.clone()))
                        }),
                    )
                    .style(ButtonStyle::Accent)
                    .content("导入配置"),
            );
            let import = rows.split_off(import_start);
            rows.push(card(import));
            rows.push(section("我的配置", "切换配置时会校验并重新连接。"));
            for profile in &snapshot.settings.profiles {
                let profile_start = rows.len();
                let id = profile.id;
                let edited_name = self.profile_names.get(&id).unwrap_or(&profile.name).clone();
                let mut label = profile.name.clone();
                if snapshot.settings.selected == Some(id) {
                    label = format!("{label} · 当前使用");
                }
                rows.push(
                    TextBlock::new()
                        .text(label)
                        .font_size(18.0)
                        .font_weight(FontWeight::SEMI_BOLD)
                        .text_wrapping(TextWrapping::Wrap)
                        .into(),
                );
                rows.push(
                    StackPanel::new()
                        .orientation(Orientation::Horizontal)
                        .spacing(8.0)
                        .children((
                            Button::new()
                                .automation_id(format!("Profile_{id}"))
                                .style(if snapshot.settings.selected == Some(id) {
                                    ButtonStyle::Accent
                                } else {
                                    ButtonStyle::Default
                                })
                                .is_enabled(available && snapshot.settings.selected != Some(id))
                                .on_click(
                                    context
                                        .callback(move |()| Message::Execute(Action::Switch(id))),
                                )
                                .content("使用配置"),
                            Button::new()
                                .automation_id(format!("Update_{id}"))
                                .is_enabled(available)
                                .on_click(context.callback(move |()| {
                                    Message::Execute(Action::UpdateProfile(id))
                                }))
                                .content("更新"),
                            Button::new()
                                .automation_id(format!("Remove_{id}"))
                                .is_enabled(available)
                                .on_click(context.callback(move |()| Message::RemoveRequested(id)))
                                .content("移除"),
                        )),
                );
                let rename_editor = StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .children((
                        TextBox::new()
                            .automation_id(format!("ProfileName_{id}"))
                            .text(&edited_name)
                            .width(240.0)
                            .is_enabled(available)
                            .on_text_changed(
                                context.callback(move |name| Message::ProfileName(id, name)),
                            ),
                        Button::new()
                            .automation_id(format!("Rename_{id}"))
                            .is_enabled(available)
                            .on_click(context.callback(move |()| {
                                Message::Execute(Action::Rename(id, edited_name.clone()))
                            }))
                            .content("重命名"),
                    ));
                rows.push(
                    Expander::new()
                        .horizontal_alignment(HorizontalAlignment::Stretch)
                        .is_expanded(self.rename_expanded.get(&id).copied().unwrap_or(false))
                        .on_is_expanded_changed(
                            context.callback(move |expanded| Message::RenameExpanded(id, expanded)),
                        )
                        .slots([
                            SlotView::new(ExpanderSlot::Header, TextBlock::new().text("重命名")),
                            SlotView::new(ExpanderSlot::Content, rename_editor),
                        ]),
                );
                let source = match config::profile_source(&profile.source) {
                    Ok(source) => source,
                    Err(error) => format!("来源无效：{error}"),
                };
                rows.push(TextBlock::new().text(source).into());
                if let Some(updated) = profile.updated_at {
                    let updated_time =
                        std::time::UNIX_EPOCH + std::time::Duration::from_secs(updated);
                    let label = match updated_time.elapsed() {
                        Ok(elapsed) => format!("上次更新：{} 分钟前", elapsed.as_secs() / 60),
                        Err(_) => "上次更新时间晚于当前系统时间".into(),
                    };
                    rows.push(TextBlock::new().text(label).into());
                }
                if self.pending_remove == Some(id) {
                    rows.push(
                        TextBlock::new()
                            .text("确认移除？当前配置被移除时将停止内核。")
                            .into(),
                    );
                    rows.push(
                        StackPanel::new()
                            .orientation(Orientation::Horizontal)
                            .spacing(8.0)
                            .children((
                                Button::new()
                                    .automation_id(format!("ConfirmRemove_{id}"))
                                    .is_enabled(available)
                                    .on_click(
                                        context.callback(move |()| {
                                            Message::Execute(Action::Remove(id))
                                        }),
                                    )
                                    .content("确认移除"),
                                Button::new()
                                    .on_click(context.callback(|()| Message::CancelRemove))
                                    .content("取消"),
                            )),
                    );
                }
                let profile_rows = rows.split_off(profile_start);
                rows.push(card(profile_rows));
            }
        }
        if self.page == Page::Nodes {
            rows.push(setting_row(
                "按延迟排序",
                "成功测速的节点优先，未测速或失败节点置后",
                ToggleSwitch::new()
                    .automation_id("SortDelayButton")
                    .is_on(self.sort_delays)
                    .on_toggled(context.callback(Message::SortDelays)),
            ));
            rows.push(
                TextBox::new()
                    .automation_id("NodeSearchBox")
                    .text(&self.search)
                    .placeholder_text("搜索节点名称")
                    .on_text_changed(context.callback(Message::Search))
                    .into(),
            );
            if let Some(live) = &snapshot.live {
                let query = self.search.to_lowercase();
                let mut matching_nodes = 0;
                for (name, group) in &live.groups {
                    if !group
                        .all
                        .iter()
                        .any(|node| node.to_lowercase().contains(&query))
                    {
                        continue;
                    }
                    let group_start = rows.len();
                    rows.push(
                        TextBlock::new()
                            .text(format!(
                                "{name} · {} · {}",
                                group.kind,
                                group.now.as_deref().unwrap_or("无当前节点")
                            ))
                            .into(),
                    );
                    let selectable =
                        ["Selector", "URLTest", "Fallback"].contains(&group.kind.as_str());
                    let delay_group = name.clone();
                    rows.push(
                        Button::new()
                            .automation_id(format!("DelayGroup_{name}"))
                            .is_enabled(available)
                            .on_click(context.callback(move |()| {
                                Message::Execute(Action::DelayGroup(delay_group.clone()))
                            }))
                            .content("整组测速"),
                    );
                    let mut sorted_nodes: Vec<_> = group.all.iter().collect();
                    if self.sort_delays {
                        sorted_nodes.sort_by_key(|node| {
                            (
                                !snapshot.delays.contains_key(*node),
                                snapshot.delays.get(*node).copied(),
                                *node,
                            )
                        });
                    }
                    let nodes: Vec<View> = sorted_nodes
                        .into_iter()
                        .filter(|node| node.to_lowercase().contains(&query))
                        .map(|node| {
                            let name = name.clone();
                            let node = node.clone();
                            let mut label = node.clone();
                            if group.now.as_ref() == Some(&node) {
                                label = format!("● {label}");
                            }
                            if let Some(delay) = snapshot.delays.get(&node) {
                                label = format!("{label} · {delay} ms");
                            }
                            if let Some(error) = snapshot.delay_errors.get(&node) {
                                label = format!("{label} · 测速失败：{error}");
                            }
                            let delay_node = node.clone();
                            let delay_id = format!("Delay_{name}_{delay_node}");
                            StackPanel::new()
                                .orientation(Orientation::Horizontal)
                                .spacing(8.0)
                                .children((
                                    Button::new()
                                        .automation_id(format!("Node_{name}_{node}"))
                                        .is_enabled(available && selectable)
                                        .on_click(context.callback(move |()| {
                                            Message::Execute(Action::Select(
                                                name.clone(),
                                                node.clone(),
                                            ))
                                        }))
                                        .content(
                                            TextBlock::new()
                                                .text(label)
                                                .max_width(232.0)
                                                .text_wrapping(TextWrapping::Wrap),
                                        ),
                                    Button::new()
                                        .automation_id(delay_id)
                                        .is_enabled(available)
                                        .on_click(context.callback(move |()| {
                                            Message::Execute(Action::Delay(delay_node.clone()))
                                        }))
                                        .content("测速"),
                                ))
                        })
                        .collect();
                    matching_nodes += nodes.len();
                    rows.push(
                        StackPanel::new().spacing(8.0).keyed_children(
                            nodes
                                .into_iter()
                                .enumerate()
                                .map(|(index, node)| (index.to_string(), node)),
                        ),
                    );
                    let group_rows = rows.split_off(group_start);
                    rows.push(card(group_rows));
                }
                if matching_nodes == 0 {
                    rows.push(section(
                        "没有匹配的节点",
                        "尝试其他关键词，或检查配置中的策略组。",
                    ));
                }
            } else {
                rows.push(
                    TextBlock::new()
                        .text("尚未连接。前往概览启动内核后，即可选择节点。")
                        .into(),
                );
            }
        }
        if self.page == Page::Logs {
            rows.push(
                TextBox::new()
                    .automation_id("LogSearchBox")
                    .text(&self.log_search)
                    .placeholder_text("搜索日志关键词")
                    .on_text_changed(context.callback(Message::LogSearch))
                    .into(),
            );
            rows.push(
                StackPanel::new().spacing(8.0).keyed_children(
                    [
                        ("all", "全部"),
                        ("info", "信息"),
                        ("warning", "警告"),
                        ("error", "错误"),
                        ("debug", "调试"),
                    ]
                    .into_iter()
                    .map(|(level, label)| {
                        (
                            level.to_owned(),
                            Button::new()
                                .automation_id(format!("LogLevel_{level}"))
                                .is_enabled(self.log_level != level)
                                .on_click(
                                    context.callback(move |()| Message::LogLevel(level.into())),
                                )
                                .content(label),
                        )
                    }),
                ),
            );
            rows.push(
                Button::new()
                    .automation_id("RefreshLogButton")
                    .is_enabled(available)
                    .on_click(context.callback(|()| Message::Execute(Action::Refresh)))
                    .content("刷新日志"),
            );
            let query = self.log_search.to_lowercase();
            let lines: Vec<_> = snapshot
                .log_text
                .lines()
                .filter(|line| {
                    (self.log_level == "all" || line.contains(&format!("level={}", self.log_level)))
                        && line.to_lowercase().contains(&query)
                })
                .collect();
            let mut text = lines.join("\n");
            if text.is_empty() {
                text = "暂无匹配日志".into();
            }
            rows.push(
                TextBlock::new()
                    .text("显示日志末尾最多 64 KiB，自动刷新。")
                    .into(),
            );
            rows.push(
                TextBlock::new()
                    .automation_id("LogContentBox")
                    .text(text)
                    .text_wrapping(TextWrapping::Wrap)
                    .into(),
            );
        }
        if self.page == Page::Settings {
            rows.push(section("订阅定时更新", "仅在应用运行时刷新 URL 订阅；失败保留原配置，按所选间隔重试。当前配置内容变化后会重新连接。"));
            let interval = snapshot.settings.subscription_interval_seconds;
            rows.push(
                StackPanel::new().spacing(8.0).keyed_children(
                    [
                        (0, "关闭"),
                        (3600, "每小时"),
                        (21600, "每 6 小时"),
                        (86400, "每天"),
                    ]
                    .into_iter()
                    .map(|(seconds, label)| {
                        (
                            seconds.to_string(),
                            Button::new()
                                .automation_id(format!("SubscriptionInterval_{seconds}"))
                                .is_enabled(available && interval != seconds)
                                .on_click(context.callback(move |()| {
                                    Message::Execute(Action::SubscriptionInterval(seconds))
                                }))
                                .content(label),
                        )
                    })
                    .collect::<Vec<_>>(),
                ),
            );
            rows.push(section("桌面行为", "按你的使用习惯设置连接与窗口行为。"));
            rows.push(setting_row(
                "开机启动",
                "登录 Windows 后以管理员权限启动应用",
                ToggleSwitch::new()
                    .automation_id("LaunchAtLoginButton")
                    .is_on(snapshot.settings.launch_at_login)
                    .is_enabled(available)
                    .on_toggled(
                        context
                            .callback(|enabled| Message::Execute(Action::LaunchAtLogin(enabled))),
                    ),
            ));
            let auto_start = snapshot.settings.auto_start;
            let close_to_tray = snapshot.settings.close_to_tray;
            let start_hidden = snapshot.settings.start_hidden;
            rows.push(setting_row(
                "启动后自动连接",
                "应用打开后连接当前配置",
                ToggleSwitch::new()
                    .automation_id("AutoStartButton")
                    .is_on(auto_start)
                    .is_enabled(available)
                    .on_toggled(context.callback(move |enabled| {
                        Message::Execute(Action::DesktopSettings {
                            auto_start: enabled,
                            close_to_tray,
                            start_hidden,
                        })
                    })),
            ));
            rows.push(setting_row(
                "关闭时保留在托盘",
                "关闭窗口后继续保持连接",
                ToggleSwitch::new()
                    .automation_id("CloseBehaviorButton")
                    .is_on(close_to_tray)
                    .is_enabled(available)
                    .on_toggled(context.callback(move |enabled| {
                        Message::Execute(Action::DesktopSettings {
                            auto_start,
                            close_to_tray: enabled,
                            start_hidden,
                        })
                    })),
            ));
            rows.push(setting_row(
                "启动时隐藏窗口",
                "通过托盘图标打开主窗口",
                ToggleSwitch::new()
                    .automation_id("StartHiddenButton")
                    .is_on(start_hidden)
                    .is_enabled(available)
                    .on_toggled(context.callback(move |enabled| {
                        Message::Execute(Action::DesktopSettings {
                            auto_start,
                            close_to_tray,
                            start_hidden: enabled,
                        })
                    })),
            ));
            let mut app_label = format!("当前版本：{}", env!("CARGO_PKG_VERSION"));
            if let Some(version) = &snapshot.app_latest_version {
                app_label = format!("{app_label} · 最新版本：{version}");
            }
            rows.push(card(vec![
                TextBlock::new().text("应用更新").font_size(20.0).into(),
                TextBlock::new()
                    .automation_id("AppUpdateStatusText")
                    .text(app_label)
                    .into(),
                TextBlock::new()
                    .text("从 GitHub 下载并校验安装包，安装后自动重启。")
                    .text_wrapping(TextWrapping::Wrap)
                    .into(),
                Button::new()
                    .automation_id("CheckAppUpdateButton")
                    .is_enabled(available)
                    .on_click(context.callback(|()| Message::Execute(Action::CheckAppUpdate)))
                    .content("检查应用更新"),
                Button::new()
                    .automation_id("UpgradeAppButton")
                    .is_enabled(available && snapshot.app_update_available)
                    .on_click(context.callback(|()| Message::Execute(Action::UpgradeApp)))
                    .content("更新并重启"),
            ]));
            let update_start = rows.len();
            rows.push(TextBlock::new().text("内核更新").font_size(20.0).into());
            let mut update_label = format!("当前版本：{}", snapshot.core_version);
            if let Some(latest) = &snapshot.latest_version {
                update_label = format!("{update_label} · 最新版本：{latest}");
            }
            rows.push(
                TextBlock::new()
                    .automation_id("UpdateStatusText")
                    .text(update_label)
                    .into(),
            );
            rows.push(
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .children((
                        Button::new()
                            .automation_id("CheckUpdateButton")
                            .is_enabled(available)
                            .on_click(context.callback(|()| Message::Execute(Action::CheckUpdate)))
                            .content("检查更新"),
                        Button::new()
                            .automation_id("UpgradeButton")
                            .is_enabled(available && snapshot.update_available)
                            .on_click(context.callback(|()| Message::Execute(Action::Upgrade)))
                            .content("升级内核"),
                    )),
            );
            let update_rows = rows.split_off(update_start);
            rows.push(card(update_rows));
            rows.push(section(
                "诊断与支持",
                "导出内容已脱敏，可用于排查运行问题。",
            ));
            let diagnostics_start = rows.len();
            rows.push(
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .children((
                        Button::new()
                            .automation_id("OpenDataButton")
                            .is_enabled(available)
                            .on_click(context.callback(|()| Message::Execute(Action::OpenData)))
                            .content("打开数据目录"),
                        Button::new()
                            .automation_id("OpenLogButton")
                            .is_enabled(available)
                            .on_click(context.callback(|()| Message::Execute(Action::OpenLog)))
                            .content("查看内核日志"),
                        Button::new()
                            .automation_id("ExportDiagnosticsButton")
                            .is_enabled(available)
                            .on_click(
                                context.callback(|()| Message::Execute(Action::ExportDiagnostics)),
                            )
                            .content("导出脱敏诊断"),
                    )),
            );
            rows.push(
                TextBlock::new()
                    .text(format!("数据和日志：{}", snapshot.directory))
                    .text_wrapping(TextWrapping::Wrap)
                    .into(),
            );
            rows.push(
                TextBlock::new()
                    .automation_id("EnvironmentText")
                    .text(format!(
                        "{} / {}",
                        std::env::var("COMPUTERNAME").unwrap_or_else(|error| error.to_string()),
                        std::env::var("USERNAME").unwrap_or_else(|error| error.to_string())
                    ))
                    .into(),
            );
            let diagnostics_rows = rows.split_off(diagnostics_start);
            rows.push(card(diagnostics_rows));
            rows.push(
                Button::new()
                    .automation_id("ExitButton")
                    .is_enabled(available && self.tray.is_some())
                    .on_click(context.callback(|()| Message::Exit))
                    .content("退出应用"),
            );
        }
        if self.page == Page::Settings {
            let merge_start = rows.len();
            rows.push(
                TextBlock::new()
                    .text("高级配置 · 合并 YAML")
                    .font_size(20.0)
                    .into(),
            );
            rows.push(
                TextBox::new()
                    .automation_id("MergeSettingsBox")
                    .text(&self.merge)
                    .accepts_return(true)
                    .text_wrapping(TextWrapping::Wrap)
                    .min_height(100.0)
                    .is_enabled(available)
                    .on_text_changed(context.callback(Message::Merge))
                    .into(),
            );
            let merge = self.merge.clone();
            rows.push(
                Button::new()
                    .automation_id("SaveSettingsButton")
                    .is_enabled(available)
                    .on_click(
                        context
                            .callback(move |()| Message::Execute(Action::SaveMerge(merge.clone()))),
                    )
                    .content("保存并应用"),
            );
            let merge_rows = rows.split_off(merge_start);
            rows.push(
                Expander::new()
                    .horizontal_alignment(HorizontalAlignment::Stretch)
                    .automation_id("AdvancedSettingsExpander")
                    .is_expanded(self.advanced_expanded)
                    .on_is_expanded_changed(context.callback(Message::AdvancedExpanded))
                    .slots([
                        SlotView::new(
                            ExpanderSlot::Header,
                            TextBlock::new().text("高级配置 · 合并 YAML"),
                        ),
                        SlotView::new(ExpanderSlot::Content, card(merge_rows)),
                    ]),
            );
        }
        let mut content_top = 20.0;
        if self.compact_navigation {
            content_top = 56.0;
        }
        let page_content = ScrollViewer::new()
            .automation_id("MainScroll")
            .horizontal_scroll_bar_visibility(ScrollBarVisibility::Disabled)
            .content(
                StackPanel::new()
                    .spacing(16.0)
                    .margin(Thickness::new(24.0, content_top, 24.0, 24.0))
                    .max_width(920.0)
                    .horizontal_alignment(HorizontalAlignment::Stretch)
                    .keyed_children(
                        rows.into_iter()
                            .enumerate()
                            .map(|(index, row)| (format!("{title}-{index}"), row)),
                    ),
            );
        let severity = if self.status_error {
            InfoBarSeverity::Error
        } else {
            InfoBarSeverity::Success
        };
        let notification = InfoBar::new()
            .automation_id("StatusText")
            .message(&self.status)
            .severity(severity)
            .is_open(self.status_visible && self.status != "就绪")
            .is_closable(true)
            .on_closed(context.callback(|()| Message::DismissStatus))
            .margin(Thickness::new(24.0, 0.0, 24.0, 12.0))
            .grid_row(1);
        let content = Grid::new()
            .rows([GridLength::Star(1.0), GridLength::Auto])
            .children((page_content, notification));
        let navigation: Vec<(String, View)> = [
            (
                Page::Overview,
                "overview",
                "OverviewTab",
                "概览",
                Symbol::Home,
            ),
            (
                Page::Profiles,
                "profiles",
                "ProfilesTab",
                "配置",
                Symbol::Document,
            ),
            (Page::Nodes, "nodes", "NodesTab", "节点", Symbol::World),
            (Page::Logs, "logs", "LogsTab", "日志", Symbol::Document),
            (
                Page::Settings,
                "settings",
                "SettingsTab",
                "设置",
                Symbol::Setting,
            ),
        ]
        .into_iter()
        .map(|(page, tag, id, label, icon)| {
            (
                tag.to_owned(),
                NavigationViewItem::new()
                    .tag(tag)
                    .automation_id(id)
                    .is_selected(self.page == page)
                    .slots([
                        SlotView::new(
                            NavigationViewItemSlot::Content,
                            TextBlock::new().text(label),
                        ),
                        SlotView::new(NavigationViewItemSlot::Icon, SymbolIcon::new().symbol(icon)),
                    ]),
            )
        })
        .collect();
        NavigationView::new()
            .automation_id("AppNavigation")
            .pane_title("mihomo-x")
            .open_pane_length(220.0)
            .pane_display_mode(NavigationViewPaneDisplayMode::Auto)
            .is_back_button_visible(NavigationViewBackButtonVisible::Collapsed)
            .is_settings_visible(false)
            .always_show_header(false)
            .is_pane_open(self.pane_open)
            .on_is_pane_open_changed(context.callback(Message::PaneOpen))
            .on_display_mode_changed(context.callback(Message::NavigationDisplay))
            .on_selected_tag_changed(context.callback(Message::Navigate))
            .slots([
                SlotView::collection(NavigationViewSlot::MenuItems, navigation),
                SlotView::new(
                    NavigationViewSlot::PaneFooter,
                    TextBlock::new()
                        .text(format!(
                            "mihomo-x {}\nMihomo {}",
                            env!("CARGO_PKG_VERSION"),
                            snapshot.core_version
                        ))
                        .margin(16.0)
                        .text_wrapping(TextWrapping::Wrap),
                ),
                SlotView::new(NavigationViewSlot::Content, content),
            ])
    }
}

fn card(rows: Vec<View>) -> View {
    Border::new()
        .background(ThemeBrush::CardBackground)
        .border_brush(ThemeBrush::CardStroke)
        .border_thickness(1.0)
        .corner_radius(8.0)
        .padding(20.0)
        .content(
            StackPanel::new().spacing(12.0).keyed_children(
                rows.into_iter()
                    .enumerate()
                    .map(|(index, row)| (index.to_string(), row)),
            ),
        )
}
fn section(title: &str, description: &str) -> View {
    StackPanel::new().spacing(4.0).children((
        TextBlock::new()
            .text(title)
            .font_size(18.0)
            .font_weight(FontWeight::SEMI_BOLD),
        TextBlock::new()
            .text(description)
            .text_wrapping(TextWrapping::Wrap),
    ))
}
fn setting_row(title: &str, description: &str, control: ToggleSwitch) -> View {
    Border::new()
        .background(ThemeBrush::CardBackground)
        .border_brush(ThemeBrush::CardStroke)
        .border_thickness(1.0)
        .corner_radius(8.0)
        .padding(16.0)
        .content(
            Grid::new()
                .columns([GridLength::Star(1.0), GridLength::Auto])
                .children((
                    StackPanel::new()
                        .spacing(4.0)
                        .margin(Thickness::new(0.0, 0.0, 16.0, 0.0))
                        .children((
                            TextBlock::new()
                                .text(title)
                                .font_weight(FontWeight::SEMI_BOLD),
                            TextBlock::new()
                                .text(description)
                                .text_wrapping(TextWrapping::Wrap),
                        )),
                    Border::new()
                        .grid_column(1)
                        .vertical_alignment(VerticalAlignment::Center)
                        .content(control.automation_name(title).slots([
                            SlotView::new(
                                ToggleSwitchSlot::OnContent,
                                TextBlock::new().text("开启"),
                            ),
                            SlotView::new(
                                ToggleSwitchSlot::OffContent,
                                TextBlock::new().text("关闭"),
                            ),
                        ])),
                )),
        )
}

fn run() -> config::Result<()> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, GetLastError},
        System::Threading::CreateMutexW,
    };
    let name: Vec<u16> = "Local\\mihomo-x-client"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // Keep this named mutex alive to prevent concurrent writers to the app data directory.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    let existing = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let _instance = unsafe { OwnedHandle::from_raw_handle(handle) };
    if existing {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow,
        };
        let title: Vec<u16> = "mihomo-x · Mihomo".encode_utf16().chain(Some(0)).collect();
        let window = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
        if window.is_null() {
            return Err("mihomo-x 正在启动，请稍后使用已有窗口".into());
        }
        unsafe {
            ShowWindow(window, SW_RESTORE);
            SetForegroundWindow(window);
        }
        return Ok(());
    }
    let job = kernel::Job::new()?;
    let progress = Arc::new(kernel::DownloadProgress::default());
    let backend = match Backend::open(job.0 as usize, progress.clone()) {
        Ok(backend) => backend,
        Err(error) => {
            let Some(settings_error) = error.downcast_ref::<config::SettingsFileError>() else {
                return Err(error);
            };
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO, MessageBoxW,
            };
            let message: Vec<u16> = format!("{settings_error}\n\n是否备份损坏设置并恢复配置列表？原 profile 文件保留，模式、TUN 和订阅来源记录会重置，自动连接关闭。选择“否”退出，不修改设置。")
                .encode_utf16().chain(Some(0)).collect();
            let title: Vec<u16> = "mihomo-x · 设置恢复"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let answer = unsafe {
                MessageBoxW(
                    std::ptr::null_mut(),
                    message.as_ptr(),
                    title.as_ptr(),
                    MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
                )
            };
            if answer != IDYES {
                return Ok(());
            }
            config::recover_settings(&settings_error.path)?;
            Backend::open(job.0 as usize, progress.clone())?
        }
    };
    App::run_component::<ClientApp>(Input(Arc::new(Mutex::new(backend)), progress))?;
    drop(job);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        let message: Vec<u16> = format!("mihomo-x 启动失败：{error}")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let title: Vec<u16> = "mihomo-x".encode_utf16().chain(Some(0)).collect();
        // These NUL-terminated strings outlive the modal native error dialog.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
        std::process::exit(1);
    }
}
