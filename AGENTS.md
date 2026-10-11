# 开发约束

- 保持简单、直接的架构，不使用 DDD，不添加没有当前需求的层、接口、包装或抽象。
- 优先线性、从上到下可读的代码，使用 guard clause 和 early return，保持相关逻辑在一起。
- 不新增或修改测试文件、测试用例、测试工具、fixtures、mocks 或测试依赖。可以运行已有检查和测试；GUI 验证通过直接调用 CLI 完成。
- 不吞掉异常，不以任意默认值隐藏错误，不削弱类型检查或 lint 规则。外部输入在边界验证。
- TypeScript 不使用三元运算符、`any`、非空断言、非布尔值的 truthiness 检查或绕过类型检查的断言；控制流嵌套不超过两层。
- 仅修改当前任务所需的代码。现有 Rust 项目和独立的 C# 验证样例不得在没有任务要求时互相替换。

# WinUI 3 开发与沙箱验证

## 执行边界

- 开发、依赖还原和编译在 Windows 宿主机完成。
- 应用运行、GUI 操作和涉及 Mihomo 的集成验证在 Windows Sandbox 内完成。
- 宿主机调用 `winapp run`、`winapp ui` 时明确指定 `--on sandbox`。省略该参数会选择宿主机，不能作为沙箱验证。
- GUI、未来的 `mihomo.exe` 和运行配置必须部署到沙箱内部。不得为了通过验证而在宿主机启动代理、修改系统代理或注册表。
- 优先使用 UI Automation 和实际 AutomationId；需要验证真实鼠标输入时使用 `winapp ui click`。
- 截图和日志保存在专用产物目录，不提交构建输出、临时配置、截图或日志到 Git。

## 当前最小应用

`sandbox-probe/SandboxProbe.csproj` 是独立的 C# / WinUI 3 样例，采用 x64、自包含、无 MSIX 包身份的方式运行，携带 .NET 和 Windows App SDK 运行时。

- `EnvironmentText` 显示机器名与用户。
- `VerifyButton` 点击后增加计数。
- `StatusText` 初始为 `Ready: 0 clicks`，操作后为 `Verified: N clicks`。
- 固定 AutomationId，便于准确定位控件。

## 环境检查与准备

在仓库根目录执行。先检查工具，不要仅凭存在 `WindowsSandbox.exe` 判断环境就绪：

```powershell
dotnet --list-sdks
winapp --version
Get-Command wsb -ErrorAction SilentlyContinue
Get-AppxPackage '*Sandbox*' | Select-Object Name,Status
winapp target snapshot sandbox --json
```

需要 .NET 10 SDK、winapp CLI 和提供 `wsb.exe` 的新版 Store 版 Windows Sandbox。Windows 的虚拟化和沙箱功能也必须可用。

本机已验证的工具组合（2026-10-11）：

- .NET SDK 10.0.401。
- winapp CLI 0.7.1。
- Windows App SDK 2.5.1。
- Windows SDK BuildTools 10.0.28000.2705。
- Windows Sandbox 客户端 0.8.107.0。

这些版本是成功记录，不代表要求自动升级其他项目。样例 NuGet 依赖已固定。

本机便携 SDK 和 CLI 位于 `$env:LOCALAPPDATA\te-toolchain`。新终端需要设置进程级 SDK 环境：

```powershell
$env:DOTNET_ROOT = "$env:LOCALAPPDATA\te-toolchain\dotnet"
$env:PATH = "$env:DOTNET_ROOT;$env:PATH"
```

本次 MSIX 版 winapp 未识别上述便携 SDK，构建部署改用同版本便携 CLI。其他 `winapp ui` 命令可以使用已安装的 CLI。缺少 winapp 时可以安装微软官方包：

```powershell
winget install --id Microsoft.WinAppCli --exact --source winget --accept-source-agreements --accept-package-agreements
```

## 构建与部署

```powershell
dotnet build .\sandbox-probe\SandboxProbe.csproj -p:Platform=x64 -p:RuntimeIdentifier=win-x64
& "$env:LOCALAPPDATA\te-toolchain\winapp\winapp.exe" run .\sandbox-probe\SandboxProbe.csproj --arch x64 --on sandbox --detach --json
```

正常安装的 SDK 可直接使用 `winapp run`。检查每个命令的退出码；构建或部署失败时停止依赖它的后续操作，先解决实际错误。

`run` 的 JSON 应标明沙箱目标。若结果提供 `UiTargetArgs` 或沙箱 PID，优先使用该作用域内的目标；否则先列出沙箱窗口，确认实际应用名。PID、窗口句柄和部署目录在每次会话中可能变化，不得复用历史值。

## GUI 验证顺序

以下命令以当前样例名为例；其他应用使用实际名称和控件标识符。

```powershell
New-Item -ItemType Directory -Force -Path .\sandbox-probe\artifacts\output | Out-Null
$env:WINAPP_UI_WORKFLOW_ID = 'te-winui-probe-session'
winapp ui list-windows --on sandbox --json
winapp ui inspect --on sandbox -a SandboxProbe --depth 8 --json
winapp ui screenshot --on sandbox -a SandboxProbe -o .\sandbox-probe\artifacts\output\before.png
winapp ui invoke VerifyButton --on sandbox -a SandboxProbe --action invoke --json
winapp ui get-value StatusText --on sandbox -a SandboxProbe --json
winapp ui click VerifyButton --on sandbox -a SandboxProbe --json
winapp ui get-value StatusText --on sandbox -a SandboxProbe --json
winapp ui inspect --on sandbox -a SandboxProbe --depth 8 --json
winapp ui screenshot --on sandbox -a SandboxProbe -o .\sandbox-probe\artifacts\output\after.png
winapp target snapshot sandbox --json
winapp ui yield --on sandbox
```

每次独立流程使用独立的 `WINAPP_UI_WORKFLOW_ID`，并在跨 shell 调用时重复设置同一个值。结束时释放 UI turn。上面的命令是操作顺序；执行时逐项检查结果，不要在失败后盲目继续。

验证必须同时具备以下证据：

1. 编译和沙箱部署成功。
2. 沙箱窗口和控件树包含目标应用及预期 AutomationId。
3. 初始状态为 0 次，UIA 调用后为 1 次，真实点击后为 2 次。若应用已被操作过，先重新启动或记录本次实际基线。
4. 应用显示沙箱账户 `WDAGUtilityAccount`；通过沙箱命令确认进程路径位于沙箱部署目录。
5. 点击前后截图成功保存到宿主机，并实际查看截图内容。
6. 最终快照中沙箱运行正常，输入和截图能力可用。

读取沙箱内进程路径：

```powershell
winapp target exec sandbox -- powershell.exe -NoProfile -Command 'Get-Process SandboxProbe | Select-Object Id,Path | ConvertTo-Json'
```

可以把控件树、调用结果、状态和快照输出为 JSON 产物。记录实际结果，不能仅根据退出码或“已发送操作”认定界面行为成功。不要创建自动化测试文件来实现上述验证。

## 已遇到的环境问题

- `sandbox_setup_incomplete` / `ClientNotInitialized`：本次原因是仅有旧版客户端，Store 包和 `wsb.exe` 尚未注册。打开 Windows Sandbox 后，Windows 自动完成新版客户端更新。等待完成后重新检查工具和目标；不要反复重试未就绪的部署，也不要自动重启宿主机。
- 控件树可读但截图或点击失败：检查远程桌面是否连接，以及沙箱窗口是否最小化。只读检查成功不能证明输入与截图可用。
- 本次恢复最小化的新版客户端后成功：先通过宿主机 `winapp ui list-windows --json` 和 `winapp ui inspect -a WindowsSandboxRemoteSession` 确认实际窗口及控件，再调用 `winapp ui invoke Minimize-Restore -a WindowsSandboxRemoteSession`。恢复后重新检查沙箱快照。不要对未确认的其他窗口操作。
- 目标不可用时不得回退到宿主机运行。确实需要人工安装、管理员权限或重启时，报告具体阻碍和已完成的工作。
- 不输出沙箱连接凭据、secret 或完整的客户端进程命令行。

## 验证范围与报告

2026-10-11 已实际通过宿主机构建、官方 `--on sandbox` 部署、控件树读取、UIA 调用、真实鼠标点击、截图回传和沙箱远程命令。状态由 0 次更新为 2 次，应用进程位于 `C:\WinApp\deployments\...`。

这不代表 Mihomo 子进程、external-controller API、系统代理或 TUN 已通过验证。后续接入时保持 `mihomo.exe` 子进程和 localhost API 的直接架构，分别验证对应行为；TUN、虚拟网卡和驱动兼容性需要单独实测。

完成后报告修改内容、实际验证结果、截图或日志位置，以及仍未验证的范围。运行适用的现有类型检查、lint 和已有测试，不新增或修改测试。

详见 `sandbox-probe/README.md`。官方参考：

- [winapp Windows Sandbox execution](https://learn.microsoft.com/en-us/windows/apps/dev-tools/winapp-cli/sandbox-execution)
- [Windows Sandbox versions](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-versions)
