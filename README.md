# mihomo-x

GitHub 仓库：[zogwosh/mihomo-x](https://github.com/zogwosh/mihomo-x)（公开）。
下载：[GitHub Releases](https://github.com/zogwosh/mihomo-x/releases)。从 v0.1.1 开始提供 Windows x64 安装包 EXE 和 SHA256。运行安装向导，默认安装到 Program Files，提供开始菜单快捷方式、可选桌面快捷方式和卸载入口。

## 开机启动与应用更新

设置页的“开机启动”独立于“启动后自动连接”。开启后注册当前用户登录触发、最高权限运行的计划任务；关闭时删除任务。启动隐藏由现有开关控制。移动应用目录后，请关闭再开启自启以更新路径。

应用启动后检查应用自身更新，设置页也可手动检查。发现更高稳定版本后点击“更新并重启”：从公开 GitHub Releases 下载安装包、验证资产 SHA256、退出应用、静默安装到当前目录并重新启动。不需要 GitHub CLI 或登录。应用数据与独立内核保留；更新日志位于数据目录的 app-update.log 和 app-update.log.installer.log。从 v0.1.3 开始，更新前备份安装目录和卸载登记，新版须在 60 秒内确认初始化及版本；安装或启动确认失败时恢复旧安装文件和登记并重启旧版。备份保留在本次 update-* 目录。

应用名称、可执行文件、图标和数据目录已改为 `mihomo-x`；新数据目录为 `%LOCALAPPDATA%/mihomo-x`，不自动迁移旧 `%LOCALAPPDATA%/te` 数据。

## GitHub 发布

更新 `Cargo.toml` 版本及 `Cargo.lock` 后，推送对应 `v<版本>` 标签。`.github/workflows/release.yml` 在 Windows 上执行现有格式与 Clippy 检查、下载已固定且校验的内核、编译和打包，使用 Inno Setup 6 编译安装包，再上传 EXE 和 SHA256 到 Release。标签必须与项目版本一致。手动运行仅生成构建产物。当前工作流仅构建 x64；ARM64 发布尚未验证。

Windows 原生 Clash（mihomo）客户端，定位为「裸包装」：不重造内核，只做界面、配置合成与内核进程管理。

基于 Rust + `windows-reactor` 开发原生 WinUI3 应用。

## 技术栈

- Rust（edition 2024）
- `windows-reactor`：WinUI3 的 Rust 绑定
- mihomo 内核（独立 `mihomo.exe` 子进程）

## 项目目标

开发 windows native 的 clash 裸包装。只提供以下功能

1. 将导入的配置文件视为 profile，app 不直接修改导入的配置文件
2. 完全尊重原本的配置文件，真正载入的配置，由设置项提供和原本的配置文件合成
3. 提供切换全局代理，tun，直连
4. 提供切换 profile，节点的能力
5. app 构建后自携带 clash mihomo 内核，并提供检查更新和升级的功能

## 架构概要

内核以**独立子进程**方式运行，app 通过 **external-controller API** 与其交互。

```
┌──────────────────────────┐        spawn / kill        ┌───────────────┐
│  te (WinUI3, Rust)       │ ─────────────────────────▶ │  mihomo.exe   │
│                          │                            │               │
│  - 配置合成 → runtime    │  HTTP  external-controller │  - 载入runtime│
│    config.yaml           │ ◀───────  (127.0.0.1) ───▶ │    config     │
│  - 启动/停止内核         │                            │  - 实际代理   │
│  - UI: profile/节点/模式 │                            └───────────────┘
└──────────────────────────┘
```

- **不直接修改导入的 profile 文件。** 用户导入的配置只读。
- 每次启动由 app 把「profile 原文 + 设置项」合成成一份 **runtime config**，写入 app 数据目录，再以 `mihomo -f <runtime-config>` 启动内核。
- 运行期可变项（如 `mode` 切换、选择节点/分组）通过 external-controller API 下发，无需重写文件。

## 设计要点

### 配置合成

- profile 原文为基线，逐字节保留，只读。
- 设置项直接 **merge** 进 profile：把设置项当作一段配置并入 profile 原文，二者合并即得 runtime config。这里没有优先级或覆盖白名单的概念，就是合并。
- runtime config 是**派生数据**：运行时按需合成，不是事实来源，可随时重建，绝不回写 profile。
- app 运行所需的注入项（`external-controller`、`secret` 等）同样通过 merge 写入 runtime config。

### 内核进程管理

- app 负责 `mihomo.exe` 的创建、退出与异常回收（app 退出时确保内核一同退出）。
- 通过 `external-controller` 与之通信：
  - 启动前将 `external-controller` 监听地址与 `secret` merge 进 runtime config。
  - external-controller 绑定 `127.0.0.1`，使用随机空闲端口 + 随机 secret，避免与用户其它 Clash 实例冲突。
  - 运行期操作（切换模式、选择节点/分组、读取流量等）走该 API。

### 内核更新 / 升级

- 构建产物自携带 `mihomo.exe`（随 app 一起分发），运行期使用的就是这份内置内核。
- 更新来源：mihomo 官方 **GitHub Releases**。检查更新即比对内置版本与最新 release。
- 升级时按当前进程架构选取对应资产，下载后替换内置内核（停内核 → 替换 → 重启）。
- 初始内核嵌入 `mihomo-x.exe`，首次运行释放到应用数据目录；升级替换这份可写的内核。保留旧内核用于启动失败时恢复。只接受官方资产且验证 Release 提供的 SHA256，不自动降级。

### profile / 节点切换

- profile 列表与导入记录由 app 管理，存于 app 数据目录。
- 切换 profile 即按新 profile 重新合成 runtime config 并(重)启动内核。
- 节点/分组切换走 external-controller API。

## 快速开始

```powershell
# 下载固定的官方 Mihomo 内核并校验 SHA256（只下载，不在宿主机运行）
./scripts/prepare-core.ps1
cargo build --release

# 生成自包含的 Rust 分发目录
./scripts/package.ps1
# 分发 dist/mihomo-x-<版本>-x64-setup.exe
```

前置条件：

- Windows：跟随 WinUI3 的最低版本要求（Windows 10 1809 / build 17763 及以上）。
- Rust MSVC 工具链、Visual Studio C++ Build Tools（含 VC++ 可再发行组件）和 Windows SDK 资源编译器，用于嵌入应用图标。
- 首次构建需要网络下载 Windows App SDK 运行时。使用 `windows-reactor-setup` 自包含部署；分发目录携带 WinUI 和 VC++ 运行时，不需要 C# 或 .NET 应用运行时。

开发和构建在宿主机完成；本仓库的运行与集成验证均在 Windows Sandbox 内完成。不要直接在宿主机执行 `cargo run` 来验证代理或 TUN。

## 使用

打开 mihomo-x.exe 时默认请求管理员权限，需要确认 Windows UAC 提示。应用默认自动启动上次选中的 profile，并恢复保存的节点选择、代理模式和 TUN 状态；没有选中的 profile 时等待选择或导入。可以在桌面设置中关闭自动连接或选择启动隐藏。自动启动失败会在界面显示错误，并保持窗口可见，可修正配置后手动启动内核。

点击窗口关闭按钮默认隐藏到系统托盘，内核继续运行；可设置为退出应用。单击托盘图标或在右键菜单选择「打开 mihomo-x」可恢复窗口；托盘也提供内核、系统代理、TUN 和模式操作。退出时清理内核与系统代理，正在执行的操作会完成后再退出。窗口位置和大小定期保存，配置选择在操作时保存。

1. 选择本地 YAML 配置文件，点击「导入配置」。导入的是逐字节保留的副本；原文件不会被修改。profile 列表可切换和移除。
2. 「合并设置」输入 YAML mapping，保存后与 profile 合成运行配置。有运行中的内核时会重新启动。
3. 启动内核，选择规则、全局代理、直连模式，并在策略组中选择节点。模式和节点选择通过 localhost API 生效，节点选择按 profile 保存。
4. TUN 开关通过 API 生效。启用 TUN 需要以管理员身份运行 te；普通权限下会提示关闭应用后右键选择“以管理员身份运行”。启动配置（包括 profile 和合并设置）开启 TUN 时也检查管理员权限。其余 TUN/DNS 配置来自 profile 或合并设置，需要配置 `auto-route`、DNS 等才能接管所需流量。驱动也必须满足内核要求。
5. 检查更新后可以升级到更高的稳定版本。下载和 API 操作在后台执行，错误显示在界面；内核日志位于界面显示的数据目录。

合并采用递归 mapping 合并，同键 mapping 继续递归，标量和序列使用并入的值；不设置覆盖白名单。合成顺序为 profile、合并设置、界面选定的模式/TUN 和应用控制器参数。首次运行没有模式/TUN 选择时保留 profile 的值。profile 原文不参与写回，runtime YAML 的格式和注释不保证保留。

模式按钮改变 Mihomo 的 `mode`；独立的“系统代理”开关让遵循 Windows 当前用户代理设置的应用使用 Mihomo 的 HTTP/mixed 端口。系统代理与 TUN 独立。停止内核或退出 app 会恢复之前的系统代理设置；异常退出后的遗留设置在下次启动时恢复。其他软件或用户后来修改的代理设置会保留。profile 中相对文件路径按内核工作目录（应用数据目录）解析。

路径输入框也接受 HTTP/HTTPS 配置 URL。“更新”从原文件或 URL 重新读取配置，失败保留旧内容。支持重命名、移除确认、节点搜索和延迟检测。日志追加写入，启动时超过 4 MiB 会保留一份历史日志。

完整开发计划与本轮验证范围见 [app 完善计划](docs/app-completion-plan.md)。该计划仍在进行，尚未完成正式发布验收。

应用使用原生 WinUI 3 NavigationView 和 Mica 背景，分为概览、配置、节点、设置四个页面。概览集中管理连接、路由模式、系统代理和 TUN；配置与策略组以卡片显示；重命名和高级 YAML 默认折叠。成功与错误通过可关闭的 InfoBar 显示。窄窗口自动使用抽屉导航。诊断导出只含允许分享的状态字段，不包含 YAML 原文、订阅 URL、节点名称、日志内容或控制器密钥。设置损坏时可先备份，再恢复 profile 列表，不自动连接。应用自身更新与公开发布暂缓，目前没有 te 的发布仓库和许可证。

需要在用户仍运行旧版时构建，可以使用独立目录：

```powershell
cargo build --release --target-dir target/completion
./scripts/package.ps1 -TargetDirectory target/completion -OutputDirectory dist/completion -Archive
```

打包生成按版本和架构命名的 ZIP，以及文件清单和 ZIP 的 SHA256。不要在用户仍运行的目录中覆盖可执行文件。

数据包含 `profiles/*.yaml`、`settings.json`、派生的 `runtime.yaml`、`mihomo.exe` 和 `mihomo.log`。无包身份时位于 Windows LocalAppData 的 `te` 子目录；调试 MSIX 身份下使用 Windows 返回的真实重定向路径，保证独立内核读取相同文件。运行配置包含随机控制器密钥，不要把它作为日志分享。

## Rust 沙箱验证

```powershell
cargo build
./scripts/package.ps1 -Configuration debug -SandboxManifest
winapp run ./dist/debug --exe mihomo-x.exe --on sandbox --detach --json
```

使用返回的沙箱 PID 操作控件，不复用旧 PID。主要 AutomationId：`ProfilePathBox`、`BrowseButton`、`ImportButton`、`StartButton`、`StopButton`、`RuleButton`、`GlobalButton`、`DirectButton`、`TunOnButton`、`TunOffButton`、`MergeSettingsBox`、`SaveSettingsButton`、`CheckUpdateButton`、`UpgradeButton`、`MainScroll`、`StatusText`、`KernelStatusText`。profile 控件为 `Profile_<UUID>`，节点控件为 `Node_<group>_<node>`。

2026-10-11 已通过 x64 沙箱实际验证：Rust WinUI 界面、配置导入与原文哈希、profile 切换、配置合成、内核子进程、模式和节点 API、真实鼠标点击、TUN 网卡与真实 HTTPS 请求、官方内核从 v1.19.31 升级到 v1.19.32，以及关闭应用清理内核和网卡。详见 [验证记录](docs/verification-2026-10-11.md)。ARM64、Windows 10、真实订阅节点及升级失败恢复路径尚未实测。

`sandbox-probe` 是之前留下的独立环境验证样例，不参与 Rust 应用的构建、分发或运行。

## 项目结构

```
build.rs             # 自包含 WinUI 运行时与内置内核
src/
  main.rs            # Rust WinUI 界面和后台操作消息
  backend.rs         # profile、设置、内核操作的直接执行流程
  config.rs          # YAML 合并、原子文件写入
  kernel.rs          # Mihomo 子进程、Job、API、官方更新
scripts/
  prepare-core.ps1   # 固定内核下载与校验
  package.ps1        # 自包含分发目录
```

`target`、`dist`、下载内核和 `artifacts` 都是本地产物，不提交到 Git。

## 许可证

待定。
