# WinUI 3 sandbox probe

独立的 C# / WinUI 3 最小应用，用于验证宿主机构建和沙箱内 GUI 操作。原仓库的 Rust 项目保持不变。

界面显示运行机器和用户，提供 `VerifyButton` 按钮，点击后将 `StatusText` 从 `Ready: 0 clicks` 更新为 `Verified: 1 clicks`。

## 构建

需要 .NET 10 SDK。应用以 x64、自包含、无 MSIX 包身份的方式运行，携带 .NET 和 Windows App SDK 运行时。

```powershell
dotnet build .\sandbox-probe\SandboxProbe.csproj -p:Platform=x64 -p:RuntimeIdentifier=win-x64
```

本次使用的 SDK 位于 `$env:LOCALAPPDATA\te-toolchain\dotnet`。在新的终端中使用它：

```powershell
$env:DOTNET_ROOT = "$env:LOCALAPPDATA\te-toolchain\dotnet"
$env:PATH = "$env:DOTNET_ROOT;$env:PATH"
```

## 新版沙箱客户端

需要 Store 版 Windows Sandbox 和 `wsb.exe`。仅存在 `WindowsSandbox.exe` 不代表满足 winapp 的执行目标要求。

```powershell
winapp run .\sandbox-probe\SandboxProbe.csproj --arch x64 --on sandbox --detach --json
winapp ui inspect --on sandbox -a SandboxProbe --depth 8
winapp ui invoke VerifyButton --on sandbox -a SandboxProbe --action invoke
winapp ui click VerifyButton --on sandbox -a SandboxProbe
winapp ui get-value StatusText --on sandbox -a SandboxProbe --json
winapp ui screenshot --on sandbox -a SandboxProbe -o .\sandbox-probe\artifacts\output\after.png
```

本机安装的 MSIX 版 winapp 在此次执行中未识别通过环境变量指定的便携 SDK；需要时使用同版本便携 CLI：

```powershell
& "$env:LOCALAPPDATA\te-toolchain\winapp\winapp.exe" run .\sandbox-probe\SandboxProbe.csproj --arch x64 --on sandbox --detach --json
```

## 本次验证结果（2026-10-11）

| 步骤 | 结果 |
| --- | --- |
| 宿主机构建 | 成功，0 警告、0 错误 |
| `winapp run --on sandbox --detach --json` | 成功，目标为 x64 Windows Sandbox |
| 沙箱进程 | PID 8340，路径 `C:\WinApp\deployments\3ba6b4e3a97ac79a\SandboxProbe.exe` |
| 运行身份 | `93B0A5B2-9A66-4 / WDAGUtilityAccount` |
| 控件树 | 识别到 `EnvironmentText`、`VerifyButton`、`StatusText` |
| UIA InvokePattern | 成功，状态从 0 次更新为 1 次 |
| 真实鼠标点击 | 成功，状态更新为 `Verified: 2 clicks` |
| 点击前后截图 | 成功，文件回传宿主机 |
| 沙箱远程命令 | 成功，读取了沙箱 Windows 版本和应用进程路径 |

使用 .NET SDK 10.0.401、winapp 0.7.1、Windows App SDK 2.5.1 和 Windows Sandbox 客户端 0.8.107.0。依赖包版本已固定。

初次启动时只有旧版沙箱客户端，winapp 返回 `sandbox_setup_incomplete`。打开 Windows Sandbox 后，Windows 自动完成 Store 客户端更新，随后官方链路正常工作。截图初次失败的原因是远程桌面窗口处于最小化状态；通过宿主机 `winapp ui invoke Minimize-Restore -a WindowsSandboxRemoteSession` 恢复窗口后成功。

本机结果文件位于 `artifacts\output`：`before.json`、`after.json`、`invoke.json`、`click.json`、`status.json`、`snapshot.json`、`before.png`、`after.png`。构建输出与临时验证产物均不进入 Git。

应用不包含 Mihomo，也不修改系统代理或注册表。这次确认的是 WinUI 3 的编译、沙箱部署、运行和 GUI 自动化链路；Mihomo 子进程、网络代理和 TUN 尚未验证。未新增或修改测试文件。

官方文档：

- [winapp Windows Sandbox execution](https://learn.microsoft.com/en-us/windows/apps/dev-tools/winapp-cli/sandbox-execution)
- [Windows Sandbox versions](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-versions)
