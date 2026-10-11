# mihomo-x 发布验证（2026-10-11）

本轮将 Rust 应用、可执行文件、图标、数据目录和分发包改名为 mihomo-x。独立 C# 沙箱样例保留原名。

创建私有仓库 zogwosh/mihomo-x，并提交现有项目。新增登录自启计划任务开关、应用启动后的更新检查、私有 Release 下载、GitHub 资产 SHA256 校验、退出后文件备份与替换、重启及失败恢复。私有更新要求安装并登录 gh 且有仓库读取权限。发现新版后由用户点击“更新并重启”。

实际检查：cargo fmt --check、cargo check --locked、cargo clippy --locked -- -D warnings、独立目录 release 构建和自包含打包均通过。未新增或修改测试。

沙箱部署成功，首次 PID 5844，窗口标题为 mihomo-x · Mihomo，账号 WDAGUtilityAccount，进程位于 C:/WinApp/deployments/303d6aacc2d79547-layout/mihomo-x.exe。

- 读取控件树，确认 LaunchAtLoginButton、CheckAppUpdateButton、UpgradeAppButton。
- UIA toggle 后创建当前用户登录计划任务，RunLevel 为 Highest，执行路径为沙箱部署目录。真实点击关闭后任务计数为 0，UI 值为 Off，设置文件 launch_at_login 为 false。
- 沙箱没有 gh，应用更新检查明确失败；未向沙箱复制宿主机 GitHub 凭据。
- 直接执行生产安装脚本，在沙箱内备份和覆盖完整分发目录，任务返回 0，重启后 PID 5792、路径和实际窗口均确认。脚本检查重启进程是否在三秒内退出。远程命令创建的子进程随命令结束被回收，因此验证改为沙箱内的一次性计划任务，验证后删除该任务。
- 截图保存并实际查看：artifacts/mihomo-x/before.png、settings.png。JSON、后续截图和最终快照位于同一产物目录，不提交。

GitHub Actions 首次未开始任何构建步骤。GitHub 原因：最近账号付款失败或 spending limit 需要增加。工作流已配置；本次使用本机验证过的产物手动发布。后续托管自动构建需要处理账号 Billing & plans 限制。

尚未实测：实际注销/重新登录触发、自启在非管理员账户的行为、跨版本私有 Release 到应用重启的完整链路、下载损坏和文件替换失败的恢复、ARM64。未重新验证 Mihomo 系统代理、TUN 或远端节点。三秒进程存活检查不是完整启动健康检查。

应用数据目录不自动迁移旧 te 数据；用户若需要保留历史配置，须另行安排迁移。宿主机未运行 GUI 或 Mihomo，未修改宿主机系统代理、注册表或开机启动设置。
