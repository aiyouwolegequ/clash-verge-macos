## v2.7.4

> [!IMPORTANT]
> 本版本专注于 macOS 服务模式的全面修复与优化，解决了服务安装、TUN 模式及流量统计的多项问题。

### 🐞 修复问题

- 修复 `service.rs` 格式化字符串 bug，导致服务连接错误信息丢失详情
- 修复 macOS `osascript` 用户取消授权 (exit code -128) 被误报为安装失败
- 修复 `init_config` 中 TUN 自动关闭检查时序竞争，增加 LaunchDaemon plist 持久化检查
- 修复卸载服务后 `Tray::update_menu()` 因核心已停止导致错误日志
- 修复全局流量统计在核心重启后数据不准确（累计值改为增量计算）
- 修复前端服务安装失败后仍继续执行 `restartCore` 的问题
- 修复 `ServiceStatus::Unavailable` 错误被静默吞掉，前端无法获知服务不可用

### 🚀 优化改进

- 解耦服务模式与 TUN 模式：服务可用即优先使用，无需开启 TUN
- `check_service_comprehensive` 增加 IPC 连接重试机制（3 次 / 300ms 间隔）
- `prepare_startup` 统一模式决策，Service 模式失败自动 fallback 到 Sidecar
- `force_reinstall_service` 增加残留 IPC socket 文件清理
- `osascript` prompt 字符串转义，防止 i18n 翻译导致命令注入
- 前端 `mutateSystemState` 增加缓存失效 + 500ms 稳定延迟
- 全链路增加 `Type::Service` / `Type::Setup` 结构化日志

---



> [!IMPORTANT]
> 关于版本的说明：Clash Verge 版本号遵循 x.y.z：x 为重大架构变更，y 为功能新增，z 为 Bug 修复。

- **Mihomo(Meta) 内核升级至 v1.19.23**

### 🐞 修复问题

- 修复系统代理关闭后在 PAC 模式下未完全关闭
- 修复 macOS 开关代理时可能的卡死
- 修复修改定时自动更新后记时未及时刷新
- 修复 Linux 关闭 TUN 不立即生效
- 修复系统代理关闭序列逻辑(防止快速退出时系统代理关闭状态没有保存)

### ✨ 新增功能

- 订阅 QR code 分享
- 新增 macOS 托盘速率显示
- 快捷键操作通知操作结果
- 软件自动更新(后台下载，下次启动自动安装)

### 🚀 优化改进

- 优化 macOS 读取系统代理性能
- 优化前端 CPU 性能
- 更健壮的服务模式与边缘状况内核恢复
- 优化白名单网络下的订阅 TLS 更新兼容性

### 👙 界面样式

- 代理组实现sticky scroll的效果