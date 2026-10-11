# WinUI 3 UI/UX 重构记录 · 2026-10-11

本次基于现有 Rust / windows-reactor 实现，修改 src/main.rs、README.md 和 docs/USAGE.md。未修改后台代理逻辑、依赖或测试。

## 界面变化

- 原生 NavigationView：概览、配置、节点、设置；使用系统 Mica 和主题画刷、原生图标、标题层级与圆角卡片。
- 概览集中展示连接状态、当前配置、规则/全局/直连、Windows 系统代理、TUN。系统代理停止时明确显示保存的下次连接设置。
- 配置使用独立卡片，当前配置有文字标记；重命名默认折叠，移除仍需确认。
- 策略组使用卡片，保留节点选择与测速；搜索无结果有明确提示。
- 桌面行为使用中文 ToggleSwitch；高级 YAML 放在设置底部的 Expander 中。
- 成功和错误使用可关闭的 InfoBar。修复程序隐藏旧提示后关闭事件误关闭新提示的时序问题。
- 窄窗口自动使用导航抽屉，选择页面后收起；为菜单按钮留出顶部空间。导航与折叠面板展开状态保存在界面状态中，不被周期刷新重置。

设计参考：[微软 WinUI 3 应用结构](https://learn.microsoft.com/en-us/windows/apps/develop/ui/windows-app-sdk-app-structure)。

## 实际验证

- cargo fmt --check、cargo check、cargo clippy -- -D warnings、独立 target/completion Release 构建通过。
- 全部应用运行、GUI 操作和内核运行位于 Windows Sandbox；winapp run 明确返回 Sandbox=true。最终应用 PID 3916，路径 C:\WinApp\deployments\91e6eecbd934d9f9-layout\te.exe，账户 WDAGUtilityAccount。
- 四页面导航、桌面开关真实点击及恢复、内核启动停止、全局模式、系统代理真实点击开关和回显、TUN 运行状态、节点搜索无结果均实际操作检查。
- 不存在的本地文件导入显示红色错误 InfoBar；正常连接保持。高级 YAML 可展开并读取编辑控件，再折叠；窄窗口抽屉可展开，导航后自动收起。
- 200% DPI 下检查 2200×1500 和 1180×1400 物理窗口；截图已实际查看。最终宽窗口四页和窄窗口截图位于 artifacts/output/uiux。
- 最终退出：te/Mihomo 进程数 0、ProxyEnable 0、MetaTunnel 网卡数 0。沙箱仍运行，输入和截图能力可用。
- dist/completion/te-0.1.0-x64.zip 已更新；271 个发行文件与 ZIP 的 SHA256 校验通过。
- 未新增或修改测试，也未替换或停止宿主机正在运行的 te。

## 产物与范围

截图：overview-final.png、profiles-final.png、nodes-final.png、settings-final.png、compact-overview-final.png、compact-settings-final.png、error-final.png、search-empty.png。

控件树与结果：final-tree.json、exit-cleanup.json、final-snapshot.json、package-check.json。

本次实测为 Windows Sandbox x64 / 200% DPI / 浅色系统主题。深色、高对比度、其他 DPI、Windows 10、ARM64 和屏幕阅读器完整流程未实测；现有真实订阅及更新失败路径验收仍按原计划保留。
