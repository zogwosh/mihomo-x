# 订阅定时更新与流量验证

设置页提供关闭、每小时、每 6 小时、每天更新，默认关闭。仅处理 URL 来源，应用运行时每约 3 秒检查到期状态。根据上次成功更新时间判断到期，失败尝试在当前会话按所选间隔重试；重启应用后到期订阅会再次尝试。逐个更新并汇总错误，不因一个订阅失败跳过其他订阅。原文件只读，失败保留旧导入配置。内容未变时只更新时间，不重连。

概览通过 Mihomo /connections 读取累计计数与活动连接数，按两次采样间隔计算速度。内核重启或计数重置后重新采样，不沿用旧速度。本次内核累计不代表全系统流量。

实际通过 cargo fmt、cargo check、Clippy（-D warnings）和 release 构建。未新增或修改测试。官方 --on sandbox 部署成功，新应用 PID 5940。

- UIA 设置每小时，沙箱设置文件保存 subscription_interval_seconds=3600。
- 导入临时直连运行配置，在沙箱启动真实 Mihomo；通过 mixed-port 7897 请求 Microsoft 网站，UI 显示下载总量 0.20 MiB、下载速度 68.8 KiB/s、上传速度 0.2 KiB/s。
- 退出后将沙箱临时 profile 的来源设为不可达的 localhost URL，更新时间设为过期；重启 PID 2088 后自动定时更新触发，界面显示请求失败，原 profile SHA256 保持 01CBC5592BD4E8CEBAE49415C3B0EC076B01DC8985C9D7E7BE47EE5210EC56D1，更新时间仍为 0。验证后恢复本地来源并关闭定时更新。
- 截图 settings.png、traffic.png、failure.png 与控件树位于 artifacts/subscription；settings.png、traffic.png 已实际查看。产物不提交。

尚未实测正常远端订阅到期成功更新、等待完整小时后的重试、多订阅并行流量与大量连接场景。未重新验证 TUN 与系统代理。宿主机仅构建，未运行 GUI 或 Mihomo。
