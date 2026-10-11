# Rust 应用验证记录 · 2026-10-11

本次交付为 Rust + windows-reactor 原生 WinUI 3 应用。C# 样例未参与构建、部署或应用运行。

## 构建与分发

- Rust 1.98.1，windows-reactor / windows-reactor-setup 0.100.0。
- Rust setup 自包含 Windows App SDK Runtime 2.4.0；分发携带 VC++ CRT。
- 固定内核 v1.19.32，x64 compatible 资产，下载 SHA256 与官方 Release 一致。
- `cargo check`、`cargo clippy --all-targets -- -D warnings`、`cargo build --release` 通过。
- 没有新增或修改测试。GUI 和内核集成验证均直接使用 winapp CLI 在 Windows Sandbox 内执行。
- 分发目录为 `dist/release`，只包含 Rust 可执行文件和 Windows 原生运行时；无需 .NET 应用运行时。

## 已实际验证

| 操作 | 观察到的结果 |
| --- | --- |
| `winapp run dist/debug --exe te.exe --on sandbox` | 沙箱进程启动；窗口标题 te · Mihomo，账户 WDAGUtilityAccount，控件 AutomationId 可读 |
| 无包身份运行 Release | 将 dist/release 复制到沙箱，使用 `winapp target exec sandbox` 执行 te.exe；原生窗口和内核正常启动 |
| 原生文件选择器 | 选择沙箱里的 YAML 文件后，ProfilePathBox 显示选定路径，导入成功 |
| 导入两个 profile | 原文件与导入副本的 SHA256 均相同，包含注释的原文保持不变 |
| 启动内核 | localhost API 就绪；GUI 位于 C:\WinApp\deployments，内核与数据均位于沙箱内 |
| 全局 / 直连 / 规则 | UIA 切为 global，真实鼠标点击切为 direct，再切回 rule；API 和状态文本吻合 |
| 节点选择 | Choice 从 DIRECT 切到 REJECT，再恢复 DIRECT；API 和连接日志确认选择生效 |
| TUN | API enable=true，Meta Tunnel 网卡 Up；日志显示 gVisor、auto-route=true |
| TUN 实际流量 | curl 不设置显式代理：REJECT 时 HTTPS 退出码 35；DIRECT 时同一 example.com 请求返回 HTTP 200、退出码 0。两次请求均有对应 Mihomo 节点日志 |
| 关闭 TUN | API enable=false，关闭 GUI 后 Meta Tunnel 网卡数量为 0 |
| 切换 profile | 运行中的 profile-one 切到 profile-two，新 API 策略组包含 Second |
| 合并设置 | mixed-port 从第二个 profile 的 17892 变为合并设置的 17893；原文哈希仍不变 |
| 无效合并输入 | 根节点为序列 `[]` 时显示错误，运行中的内核保持运行 |
| 检查更新 | 官方 GitHub 返回 v1.19.32，当前版本相同时不启用升级 |
| 实际升级 | 沙箱单独放入并校验官方 v1.19.31；界面升级下载官方 v1.19.32，SHA256 和可执行版本校验通过，替换后重启，API 显示 v1.19.32 |
| 异常退出 | 在沙箱结束 Mihomo 进程，界面显示异常退出及已停止，随后可重新启动 |
| 正常退出 | 关闭 GUI 后 te 和 mihomo 进程数量均为 0，TUN 网卡数量为 0 |
| 截图 | 前后截图已回传到宿主机并实际查看 |

临时 MSIX 身份验证中曾发现 AppData 文件虚拟化导致独立子进程读取不同位置。最终实现通过 `SHGetKnownFolderPath` 的 `KF_FLAG_RETURN_FILTER_REDIRECTION_TARGET` 取得真实路径；随后包身份和无包身份运行均验证成功。

## 本地产物

`artifacts/output` 不提交 Git，包含：

- `before.png`、`after.png`、`after-upgrade.png`：已查看的前后截图。
- `final-tree.json`、`file-picker-tree.json`：实际控件树。
- `final-api.json`：已脱敏的 API、profile 哈希和进程路径记录。
- `exit-cleanup.json`：正常退出后的进程与网卡数量。
- `mihomo.log`：升级后内核运行日志。不保存包含控制器密钥的 runtime YAML。

## 未实测范围

ARM64、Windows 10、真实订阅及远端代理节点、其他 TUN 栈、非管理员权限情形、下载校验失败及升级启动失败后的恢复流程。全局按钮切换 Mihomo mode；当前没有 Windows 系统代理开关。

## 官方参考

- [Windows Reactor](https://github.com/microsoft/windows-rs/blob/master/docs/crates/windows-reactor.md)
- [Mihomo APIs](https://wiki.metacubex.one/api/)
- [Mihomo v1.19.32](https://github.com/MetaCubeX/mihomo/releases/tag/v1.19.32)
- [winapp Sandbox execution](https://learn.microsoft.com/en-us/windows/apps/dev-tools/winapp-cli/sandbox-execution)
- [Known folder flags](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/ne-shlobj_core-known_folder_flag)
