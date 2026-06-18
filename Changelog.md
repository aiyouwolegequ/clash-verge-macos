## v2.8.1

> [!IMPORTANT]
> 本版本修复延迟测试、首页网站测速、代理页排序刷新和 macOS 直连应用选择逻辑，移除自动切换节点行为，提升手动测速与 TUN 场景下的可预期性。

### 🐞 修复问题

- **首页网站测速修复**：网站延迟测试改为通过本地 Mihomo mixed 代理发起请求，避免直连网络不可达时全部显示 timeout。
- **当前节点测速修复**：首页「当前节点」手动测速只测试当前节点，不再误触发整组测速；延迟 URL 与超时阈值统一使用用户配置。
- **代理页排序刷新修复**：延迟缓存更新后会重新计算按延迟排序结果，避免测完延迟后列表仍停留在旧排序。
- **macOS 直连应用修复**：打开对话框不再自动全选所有应用；勾选改为本地暂存，点击「确定」后一次性保存并应用，清空列表也会刷新运行时规则。
- **数字配置兜底修复**：默认延迟超时和 TUN MTU 输入非法值时分别回落到 `10000ms` 和 `1500`，避免保存 `NaN` 造成异常判断。
- **服务二进制复制修复**：服务下载/解包失败时不再静默继续，空响应和临时目录残留会正确报错并清理。

### 🚀 优化改进

- **移除自动切换节点**：彻底移除代理页基于延迟的自动测试与自动切换，手动选择的节点不再被后台逻辑改写。
- **手动测全部延迟语义明确**：手动「测全部延迟」继续调用 Mihomo group delay，因此仍会取消 fixed 状态，并把核心返回的成功延迟同步到前端缓存。
- **延迟监听稳定性**：延迟管理器支持同一分组多个监听器，单节点测速也会通知分组刷新，减少不同视图之间的状态不同步。
- **应用事件监听清理**：应用数据 Provider 在异步监听注册晚于组件卸载时会立即释放监听器，减少潜在事件泄漏。

### ✅ 验证

- 完成 `pnpm prebuild --force`，Stable Mihomo 使用 `v1.19.25`，Alpha 使用 `alpha-8e2aba4`，Service 使用 `v2.3.0`。
- 完成 `cargo check`、`pnpm typecheck`、`pnpm lint`、`pnpm format:check` 验证。

---

## v2.8.0

> [!IMPORTANT]
> 本版本修复最新 Stable Mihomo 内核兼容性问题，避免因 v1.19.27 配置字段变化导致代理模式、节点信息和代理组加载失败；同时将发布构建收敛为只产出 DMG。

### 🐞 修复问题

- **Mihomo v1.19.27 配置兼容**：兼容上游移除 `global-client-fingerprint` 后的 `/configs` 响应，避免基础配置读取失败导致代理模式不可用。
- **节点与代理组加载修复**：代理节点运行时字段缺失时使用默认值，并对未知 `ProxyType` / `VehicleType` 增加兜底，避免新内核返回新类型或省略字段时整批节点、代理组解析失败。
- **格式检查修复**：让 Biome 遵循 Git ignore 规则，避免 `pnpm format:check` 扫描 `crates/tauri-plugin-mihomo/permissions/schemas/schema.json` 等既有生成文件。

### 🚀 优化改进

- **内核版本回退与可控升级**：Stable Mihomo 默认固定为 2.7.15 已验证可用的 `v1.19.25`，同时保留 `MIHOMO_VERSION`、`MIHOMO_USE_LATEST=1` 和 `MIHOMO_ALPHA_VERSION` 供后续验证或临时切换。
- **发布产物收敛**：本地 `pnpm build`、Release Build 和 Autobuild workflow 均改为 `-b dmg`，只构建 macOS DMG 产物；Release workflow 不再生成 updater JSON，Tauri 配置关闭 updater artifacts。
- **构建脚本可靠性**：本地 `pnpm build` 改为仅在 Tauri 构建成功后重命名 DMG，避免构建失败被后续脚本吞掉。

### ✅ 验证

- 完成 `pnpm prebuild --force`，Stable Mihomo 使用 `v1.19.25`，Alpha 使用 `alpha-8e2aba4`。
- 完成 `cargo fmt --check`、`cargo check`、`cargo check -p tauri-plugin-mihomo`、`cargo test -p tauri-plugin-mihomo models::tests`、`cargo test -p tauri-plugin-mihomo export_bindings` 验证。
- 完成 `pnpm typecheck`、`pnpm lint`、`pnpm format:check` 验证。
- 完成 `pnpm build`，生成 `target/aarch64-apple-darwin/release/bundle/dmg/Clash_Verge_2.8.0_aarch64.dmg`（约 59MB），SHA256：`5c5336eda32243a5a37cc733be647f91a29f7fe9a889c6c07e3d80cbface3414`。

---

## v2.7.19

> [!IMPORTANT]
> 本版本修复代理页、首页当前节点和 Clash 信息在延迟测试、节点切换或核心短暂刷新期间偶发空白的问题，并更新 Mihomo 内核与 GeoData 资源。

### 🐞 修复问题

- **代理状态刷新稳定性**：核心数据短暂不可用时保留最后一次有效代理和 Clash 配置，避免首页当前节点、Clash 信息和代理列表被短暂空响应清空；若空数据持续超过 5 秒，则恢复显示真实空态，避免掩盖配置问题。
- **延迟测试刷新顺序修复**：批量延迟测试不再在 `delayManager` 和 Mihomo `delayGroup` 竞速时提前刷新，改为等待本地和核心延迟状态都 settle 后再刷新，减少延迟排序和节点选择状态抖动。
- **刷新事件节流修复**：拆分 `profile-changed` 与 `verge://refresh-proxy-config` 的节流状态，避免配置切换时代理刷新事件被 profile 事件吞掉。
- **链式代理状态刷新修复**：链式代理连接成功后等待代理数据刷新完成并捕获刷新失败，避免连接状态偶发不同步。
- **空白卡片兜底**：Clash 信息卡片在配置短暂不可用时继续显示字段占位，当前节点卡片在已有节点数据时不再因 pending 状态渲染空白。

### 🚀 更新

- **Mihomo 内核更新**：Stable 更新至 `v1.19.27`，Alpha 更新至 `alpha-2c6ff72`。
- **GeoData 更新**：刷新 `Country.mmdb`、`geoip.dat` 和 `geosite.dat` 资源。

### ✅ 验证

- 完成 `cargo fmt --check`、`cargo check`、`cargo test -p clash-verge --lib` 验证。
- 完成 `pnpm typecheck`、`pnpm lint`、`pnpm format:check` 验证。
- 完成 `pnpm prebuild --force` 与 `pnpm prebuild`，资源更新成功。
- 完成 `pnpm build`，生成 `target/aarch64-apple-darwin/release/bundle/dmg/Clash_Verge_2.7.19_aarch64.dmg`，SHA256：`3033f7de0885c3a18475f17f5b083eab217a80ceb05ee8963fdd8bd6497cf090`。

---

## v2.7.18

> [!IMPORTANT]
> 本版本修复 v2.7.17 在启用系统代理或本地代理更新订阅时，因本地 DNS 无法解析域名导致更新失败的问题。

### 🐞 修复问题

- **代理下订阅更新修复**：当启用本地代理或系统代理更新订阅时，跳过本地 DNS 解析（跳过 `lookup_host`），交由代理端直接解析，避免在本地 DNS 受污染/被拦截时导致订阅拉取失败。
- **直连订阅校验保留**：当未使用代理（直连）更新订阅时，继续执行本地 DNS 解析并绑定解析出的公网 IP，以防止 DNS Rebinding 等出站安全风险。

### ✅ 验证

- 完成 `cargo fmt --check`、`cargo test`、`cargo check` 验证。
- 完成 `pnpm build` 编译并输出 DMG。

---

## v2.7.17

> [!IMPORTANT]
> 本版本修复 v2.7.16 安全加固后引入的订阅更新回归问题，恢复常见订阅链接的跳转下载和代理下载能力，同时保留出站目标安全校验。

### 🐞 修复问题

- **订阅重定向修复**：远程订阅下载支持最多 10 次 `http` / `https` 重定向，并在每次跳转后重新校验目标地址，修复部分订阅链接返回 302 后无法更新的问题。
- **代理更新修复**：恢复使用 Clash 代理和系统代理更新订阅的能力，修复部分订阅必须经代理访问时更新失败、节点列表为空的问题。
- **安全边界保持**：订阅下载继续拒绝本机、私网、特殊地址以及非 `http` / `https` 跳转，避免回退到不受控的出站请求行为。

### ✅ 验证

- 完成 `cargo fmt --check`、`cargo test -p clash-verge --lib`、`cargo check`、`cargo clippy --all-targets -- -D warnings`、`pnpm typecheck`、`pnpm lint`、`pnpm format:check` 验证。
- 完成 `pnpm build`，生成 `target/aarch64-apple-darwin/release/bundle/dmg/Clash_Verge_2.7.17_aarch64.dmg`，SHA256：`0d94c0f5b856cce1bbab5ce7a95423a06b360d79187cde55ac62e8828dbb053e`。

---

## v2.7.16

> [!IMPORTANT]
> 本版本聚焦安全加固，修复 Codex Security 扫描发现的 5 个问题，强化 URL 出站请求、WebDAV 传输、本地备份文件名、服务安装脚本和外部链接打开边界。

### 🔒 安全修复

- **服务安装命令加固**：安装 / 卸载服务时同时进行 Shell 参数转义与 AppleScript 字符串转义，防止路径中的引号或反斜杠突破管理员授权脚本。
- **URL 出站请求防护**：订阅、图标下载和自定义延迟测试统一校验目标地址，禁止本地 / 私网 / 特殊地址，关闭重定向，固定 DNS 解析结果，并禁止代理绕过目标校验。
- **WebDAV 传输加固**：保存配置和运行时客户端均强制使用 `https://`，拒绝旧配置中的明文 HTTP 地址，并阻止 WebDAV 重定向降级到 HTTP。
- **本地备份文件名校验**：删除、恢复、导出和创建本地备份时仅允许单层 `.zip` 文件名，拒绝路径分隔符、`..` 和非备份文件扩展名。
- **外部链接打开收敛**：个人配置列表的主页链接改为通过后端 `open_web_url` 打开，只允许 `http` 和 `https` scheme，避免直接调用 Tauri shell open。

### ✅ 验证

- 完成 Codex Security focused re-scan，5 个问题均确认修复。
- 完成 `cargo test -p clash-verge --lib`、`cargo check -p clash-verge`、`pnpm typecheck` 验证。

---

## v2.7.15

> [!IMPORTANT]
> 本版本大幅提升了配置切换（秒切）速度，更新 Mihomo 内核至最新版，彻底清除了流量统计模块，并默认开启当前节点延迟自动检测，修复了规则页面空白的问题。

### 🚀 优化改进

- **配置切换（秒切）优化**：引入 Fast-Path API 重载机制。切换配置时优先尝试通过 API 直接重载配置，重载成功仅需数毫秒，免去 sidecar 进程重复初始化的开销，实现真正“秒切”。若 API 重载失败，则自动回退至验证与核心重启流程，并配有备份自动恢复机制，确保绝对安全。
- **默认启用自动延迟检测**：节点面板默认开启自动延迟检测功能，默认检测间隔为 5 分钟，帮助用户在主页和菜单中保持最新的延迟显示。
- **内核升级**：升级 Mihomo 核心内核至最新 Stable `v1.19.25` 及 Alpha `alpha-38cb06d`。
- **模块精简**：彻底删除应用流量统计功能（包含数据库、迁移任务、后台驻留守护进程以及前端监控界面），进一步减轻 CPU 和内存开销。

### 🐞 修复问题

- **修复规则空白**：本地补丁 patch `tauri-plugin-mihomo` 以支持 `ProcessPathWildcard` 规则类型的 Serde 反序列化，彻底修复部分订阅导入后“规则”页面显示为空、搜索无效的问题。
- **内核下载路径修复**：修复 `prebuild.mjs` 在部分特殊网络环境下无法按优先级拉取 Alpha 和 Stable 内核下载链接导致 prebuild 失败的问题。

---

## v2.7.14

> [!IMPORTANT]
> 本版本优化了构建和工作流配置，使项目完全专一地针对 macOS ARM (Apple Silicon) 架构，优化了 prebuild 工具和 GitHub CI 工作流。

### 🚀 优化改进

- **限制至 macOS ARM**：Tauri 构建配置 `package.json` 添加 `--target aarch64-apple-darwin`，显式指定 Apple Silicon 目标架构。
- **本地开发优化**：优化 `scripts/prebuild.mjs` 中的平台和架构检测逻辑。在 macOS 环境下本地运行 prebuild 会强制下载/安装 `darwin-arm64` 和 `aarch64-apple-darwin` sidecars，即使在 Intel Mac 上开发也可以无缝生成 ARM 版本。
- **GitHub 工作流优化**：简化 `.github/workflows/lint-clippy.yml` 与 `.github/workflows/cargo-audit.yml`，移除针对 Windows 和 Linux 平台构建矩阵，降低 CI 运行时间并节省计算资源。

---

## v2.7.12

> [!IMPORTANT]
> 本版本优化 macOS 应用流量统计归因与「macOS 直连应用」规则生成逻辑，让同一 App 的主进程、Helper 进程和域名明细更稳定地归并到同一个应用身份下。

### 🚀 优化改进

- **应用流量统计归因**：读取 `.app/Contents/Info.plist`，基于 `bundle_id`、Bundle 路径、可执行文件名生成稳定 `app_id`
- **应用聚合准确性**：主进程、Helper 进程和路径回退数据按应用身份聚合，减少同一 App 被拆成多条记录
- **域名明细查询**：应用流量详情优先按 `app_id` 查询，并兼容旧版 `process_path` 历史数据
- **流量模式标准化**：后端存储统一为 `direct` / `reject` / `tun` / `proxy`，前端继续显示「直连 / 拦截 / TUN / 代理」
- **macOS 应用识别**：应用选择器扩展扫描 `/Applications`、`/System/Applications`、`/System/Applications/Utilities` 和 `~/Applications`，并展示 `bundle_id`
- **直连规则生成**：为选中的 `.app` 注入 `PROCESS-PATH-WILDCARD` 和 `PROCESS-NAME` 直连规则，并启用 `find-process-mode: always`
- **配置即时生效**：刷新 macOS 直连应用可执行文件后同步触发核心配置更新
- **开发校验**：修复 `pnpm format:check` 扫描 `.pnpm-store` 临时缓存导致格式检查失败的问题

### 🐞 修复问题

- 修复同一 macOS App 因 Helper 进程、可执行文件名或路径差异导致流量统计拆分的问题
- 修复直连应用仅依赖 TUN 进程排除字段时规则命中不够稳定的问题

---

## v2.7.11

> [!IMPORTANT]
> 本版本全面移除 Windows/Linux 跨平台冗余代码，精简为纯 macOS Apple Silicon 构建；同步引入上游性能优化与多项订阅/托盘/运行时稳定性修复。

### 🚀 优化改进

- **纯 macOS 构建**：彻底移除 Windows/Linux 跨平台死代码，前后端代码库体积显著精简
- **启动性能**：Monaco 编辑器改为懒加载，减少首屏 bundle 体积与启动耗时
- **运行时性能**：Tokio 线程池上限与阻塞线程数优化，降低长尾延迟
- **托盘速率渲染**：重构 `delay_timer` 为原生 Tokio 定时调度，优化托盘富文本基线偏移与行高计算
- **后端日志安全**：订阅 URL 统一脱敏处理，日志中不再泄露敏感地址

### 🐞 修复问题

- 过旧 TLS 1.0/1.1 协议订阅导入时给出明确错误原因
- gzip 压缩订阅响应被当作无效 YAML 导致导入失败
- 系统代理守护停止时未正确释放代理 guard
- 代理组 sticky scroll 滚动位置异常
- 轻量模式退出清理逻辑在 `ExitRequested` 阶段执行
- 允许 LAN 设置对话框网络接口加载失败
- YouTube Premium 解锁检测逻辑改进
- Rust 1.97 clippy 合规修复
- 前端 lint 违规清理（styled 组件、未用 state、变量重命名）

---

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
