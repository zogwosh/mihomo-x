# Windows 安装包验证

v0.1.1 使用 Inno Setup 6.7.3 生成 x64 setup.exe 和 SHA256；Release 不再发布 ZIP。安装到 Program Files，提供开始菜单快捷方式、可选桌面快捷方式和 Windows 应用卸载入口。公开仓库更新改为直接 HTTP 请求，不再依赖 gh。旧版 v0.1.0 用户需要手动安装本次版本，后续版本使用安装包更新。

实际验证：cargo fmt --check、cargo check --locked、cargo clippy --locked -- -D warnings、release 编译和 Inno Setup 编译通过。未新增或修改测试。

沙箱 GUI 安装通过，安装目录为 C:/Program Files/mihomo-x。开始菜单快捷方式与卸载注册项确认存在，版本为 0.1.1。完成安装后应用 PID 9032，界面显示 0.1.1，匿名 GitHub 更新检查成功。

退出应用后执行卸载，返回成功；应用 EXE、快捷方式、卸载记录均移除，用户数据目录保留。执行生产更新脚本静默安装并重启，脚本日志为 Installer update completed，重启 PID 1472，路径为 Program Files 内的 mihomo-x.exe，任务返回 0。

截图实际查看：artifacts/installer/wizard.png、finished.png、app.png。沙箱日志与控件树保存在 artifacts/installer，未提交构建或验证产物。

最初托管构建失败原因为 Chocolatey 没有 6.7.3 包。已改用官方 GitHub 下载及固定 SHA256，哈希参考 Microsoft winget-pkgs 中该版本的安装清单。

未实测：跨版本完整下载更新、ARM64、启动失败后的恢复、安装中断和系统重启场景。安装包未签名。桌面快捷方式选项已实现但未通过 GUI 勾选验证。卸载时清理本应用的登录计划任务，用户配置保留。
