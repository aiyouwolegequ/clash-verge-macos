## v2.7.1

### 🐞 修复问题

- 修正 `productName` 回 `Clash Verge`，避免应用名称带下划线导致与旧版 `Clash Verge.app` 共存时产生混淆

### ✨ 新增功能

- 新增构建后自动重命名脚本 `scripts/rename-dmg.mjs`，在保持 app 名称为 `Clash Verge.app` 的同时，使 DMG 文件名使用下划线格式（`Clash_Verge_2.7.1_aarch64.dmg`）

## v2.7.0

### 🐞 修复问题

- 修复拖放导入订阅后配置未自动重载的问题（拖放后调用 `enhanceProfiles()`）

### ✨ 新增功能

- **Enhance 管道内置规则去重**：新增 `deduplicate_rules`，在配置生成阶段自动删除重复的字符串规则，保留首次出现的顺序
- **Script 增强异步化与超时保护**：将 `use_script` 改为异步执行（`spawn_blocking` + 5 秒超时），防止 JS Script 死循环阻塞 Enhance 管道
- 清理非 aarch64 平台文件：移除 x86_64-apple-darwin sidecar、Windows/Linux 资源及配置，仅保留 Apple Silicon (aarch64) 支持

### 🚀 优化改进

- **Mihomo 内核升级**：
  - 稳定版更新至 `v1.19.23`
  - Alpha 版更新至 `alpha-6c407f0`
- 增加 Boa JS 引擎 `MAX_LOOP_ITERATIONS = 10_000_000` 限制，防止死循环

### 📝 技术细节

- `cargo test -p clash-verge` 新增 3 个去重相关单元测试，全量 35 个测试通过

## v2.6.2

### 🚀 优化改进

- **App 流量统计精确度提升**：
  - 新增 `global_traffic` 表，记录 Mihomo 全局流量累计值，提供准确的流量基准数据
  - 优化进程识别逻辑：当 `processPath` 为空时，优先使用 `process` 字段作为备选标识
  - 添加退避机制：Mihomo 连接失败时自动延长轮询间隔（3s → 30s），恢复后立即回到 3s
  - 缩短默认轮询间隔从 5 秒降至 3 秒，减少连接关闭时的流量丢失
- **新增全局流量查询接口**：`get_global_traffic_stats` 命令可获取指定时间段内的准确全局流量

### 📝 技术细节

- 新增 `GlobalTrafficStat` 结构体：`{ upload_bytes, download_bytes }`
- `get_connections()` API 返回的 `upload_total`/`download_total` 为 Mihomo 内部累计值，可作为流量基准
- App 流量统计仍存在少量丢失（Mihomo 连接关闭到下次轮询间隔内的流量），但可通过全局流量进行核对修正

## v2.5.9

### ✨ 新增功能

- **macOS 直连应用自动刷新**：新增 `MacExcludeAppsManager` 模块，实现每日中午 12:00 自动遍历加白应用的 Contents/MacOS 目录，确保应用更新后依然生效
- **手动刷新按钮**：在 macOS 直连应用设置界面添加刷新按钮，用户可随时手动触发刷新

### 🚀 优化改进

- **性能优化**：配置增强过程中使用预计算的 executable 列表，避免每次生成配置时的磁盘 I/O 操作
- **功能增强**：枚举 macOS 应用的完整可执行文件列表（包括 WebKit、WebKitNetworkProcess、CloudKit 等辅助进程），确保所有相关进程都能正确直连

## v2.5.8

### 🚀 优化改进

- **代码清理与规范化**：移除了冗余的死代码和注释，统一了 Rust 异步处理模式。
- **性能优化**：
  - 优化了定时器系统的锁竞争问题，采用批量写入机制。
  - 减少了配置增强过程中的内存复制（Clone）开销。
- **安全性提升**：完善了 WebDAV 服务器自签名证书的处理逻辑说明。
- **问题修复**：修正了服务初始化中的字符串插值错误，移除了 DNS 配置中的重复项，并纠正了超时的描述文档。

## v2.5.7

### 🚀 优化改进 (从上游合并)

- **后端优化**：
  - 改进轻量模式窗口关闭与焦点监听管理 ([#upstream](https://github.com/clash-verge-rev/clash-verge-rev))
  - 优化延迟测试逻辑：非代理环境下跳过代理请求，HTTP 测试改用 HEAD 请求。
- **构建优化**：
  - **支持 macOS 交叉编译 Windows x64**：集成 `cargo-xwin` 与 `nsis` 环境，实现在 macOS 下直接构建 Windows 安装包。
- **前端优化**：
  - 系统代理与 TUN 切换实现乐观更新，响应更迅速。
  - 引入任务队列处理频繁切换开关时的竞态问题。

## v2.5.6

### 🐞 修复问题

- 修复应用流量统计的时间记录逻辑（改为当日 00:00:00 开始记录）
- 修改本周和本月流量统计逻辑：本周（周一至周日），本月（按自然月调整天数）
- 更新应用版本号为 2.5.6

## v2.4.7

### 🐞 修复问题

- 修复 Windows 管理员身份运行时开关 TUN 模式异常
- 修复静默启动与自动轻量模式存在冲突
- 修复进入轻量模式后无法返回主界面
- 切换配置文件偶尔失败的问题
- 修复节点或模式切换出现极大延迟的回归问题

<details>
<summary><strong> ✨ 新增功能 </strong></summary>

</details>

<details>
<summary><strong> 🚀 优化改进 </strong></summary>

- 优化订阅错误通知，仅在手动触发时
- 隐藏日志中的订阅信息
- 优化部分界面文案文本
- 优化切换节点时的延迟
- 优化托盘退出快捷键显示
- 优化首次启动节点信息刷新
- Linux 默认使用内置窗口控件
- 实现排除自定义网段的校验
- 移除冗余的自动备份触发条件

</details>
