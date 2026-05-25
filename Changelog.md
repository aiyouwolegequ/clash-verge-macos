## v2.7.10

> [!IMPORTANT]
> 本版本进一步优化应用流量统计中的应用名称可读性，彻底去除 `.app` 后缀，规范全小写应用名的大小写，并清理更多版本号格式。

### 🚀 优化改进

- 应用名称彻底去除 `.app` / `.APP` 后缀（如 `Google Chrome.app` → `Google Chrome`）
- 全小写应用名自动规范化大小写（如 `google-chrome-stable` → `Google Chrome Stable`，`curl` → `Curl`）
- 支持清理 `v` 前缀版本号（如 `Clash Verge v2.7.9` → `Clash Verge`）
- 后端数据入库时同步去除 `.app` 后缀，从源头保证数据干净

---

## v2.7.9

> [!IMPORTANT]
> 本版本优化应用流量统计中的应用名称可读性，正确提取 .app Bundle 名并去除尾部版本号。

### 🚀 优化改进

- 修复 `.app` 路径在中间时无法正确提取 Bundle 名称的问题（如 `Software Update.app/Contents/MacOS/softwareupdate` → `Software Update`）
- 应用名尾部版本号自动清理（如 `Google Chrome 128.0.6613.138` → `Google Chrome`）

---

## v2.7.8

> [!IMPORTANT]
> 本版本修复 React 19 实验性 `use` hook 可能导致的生产构建运行时错误。

### 🐞 修复问题

- 将 React 19 实验性 `use` hook 替换为稳定的 `useContext`，消除潜在的 hooks 计数不匹配问题（Minified React error #310）
- 备份设置功能异常
- macOS 托盘速率可能的样式错误
- 修复 订阅导入 TLS 1.0/1.1 等过旧协议时显示更明确错误原因
- 修复 gzip 压缩订阅响应被当作无效 YAML 导致导入失败的问题

<details>
<summary><strong> ✨ 新增功能 </strong></summary>

---

## v2.7.7

> [!IMPORTANT]
> 本版本修复 macOS TUN 模式应用流量统计展示真实应用名称，并支持点击应用查看域名级流量明细。

### ✨ 新增功能

- 应用流量统计支持点击具体应用查看域名级流量明细
- macOS TUN 模式下通过 `lsof` 反向解析真实应用名称（替代域名回退）

---

## v2.7.6

> [!IMPORTANT]
> 本版本新增代理节点自动延迟测试与故障切换，优化版本号管理，并在代理模式页面集成 macOS 直连应用快捷入口。

### ✨ 新增功能

- 代理节点默认按延迟排序
- 每30秒自动测试代理节点延迟
- 当前节点 timeout 时自动切换为同组延迟最低节点
- macOS 直连应用设置入口迁移到代理模式页面
- release-version 脚本支持 `patch` / `minor` / `major` 自动进位

### 🚀 优化改进

- Mihomo(Meta) 内核升级至 v1.19.24

---

## v2.7.5

> [!IMPORTANT]
> 本版本在上个版本基础上进一步修复了应用流量统计问题，增加了模式筛选和列排序能力，统一修复了版本号标注，并升级了 Mihomo 内核。

- **Mihomo(Meta) 内核升级至 v1.19.24**

### 🐞 修复问题

- 修复应用流量统计在 macOS TUN 模式下 process/path 为空导致数据缺失（回退到 host/remote_destination 分组）
- 修复全局流量统计在核心重启后累计值归零（改为增量持久化到 SQLite）
- 修复应用流量统计首次轮询产生 burst 脏数据（增加 is_first_poll 基准建立机制）

### ✨ 新增功能

- 应用流量统计支持按模式（直连/拦截/TUN/代理）筛选
- 应用流量统计支持上传、下载、合计列排序

---

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
- 修复 IPC 协议不匹配导致的 JSON 序列化错误（锁定 `clash_verge_service_ipc` 至 v2.3.0）
- 修复 macOS 服务安装阻塞 UI（改为 async + 60s 超时）
- 修复 TUN 模式自动关闭时与配置验证的并发冲突（增加 2s 延迟重试）
- 修复切换代理节点后 IP 信息不刷新（增加 React Query 缓存失效）
- 修复 prebuild 网络强依赖导致构建中断（sidecar 已存在时跳过版本检查）

### 🚀 优化改进

- 解耦服务模式与 TUN 模式：服务可用即优先使用，无需开启 TUN
- `check_service_comprehensive` 增加 IPC 连接重试机制（3 次 / 300ms 间隔）
- `prepare_startup` 统一模式决策，Service 模式失败自动 fallback 到 Sidecar
- `force_reinstall_service` 增加残留 IPC socket 文件清理
- `osascript` prompt 字符串转义，防止 i18n 翻译导致命令注入
- 前端 `mutateSystemState` 增加缓存失效 + 500ms 稳定延迟
- 全链路增加 `Type::Service` / `Type::Setup` 结构化日志
- `prepare_startup` TUN 服务等待时间延长至 12s（40 次 × 300ms）
- `handle_service_status` 安装服务后使用 `wait_and_check_service_available` 等待就绪
- 服务二进制文件改为从官方 releases 下载 v2.3.0 真实 daemon（替代 stub）

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
- 修复 Linux 快捷键映射错误

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
- 代理组实现 Sticky Scroll 效果
