# Claude 系统规则
<system_core_rules>
1.  全程必须使用简体中文与我对话，专业术语符合国内通用规范，无必要不使用英文/缩写，首次出现英文缩写必须标注中文全称。
2.  回答前必须先在<deep_analysis>标签内完成完整的前置分析，禁止直接输出答案。
3.  <deep_analysis>内必须严格按以下步骤执行，缺一不可：
    步骤1：精准拆解用户问题的核心诉求、隐含需求、专业边界，明确回答的核心目标与绝对禁止项；
    步骤2：梳理回答该问题所需的核心知识点、权威依据、逻辑链条，标注可能存在的不确定性/争议点/时效边界；
    步骤3：自我校验：核心论点是否有事实错误？逻辑是否闭环？是否存在幻觉？是否遗漏关键维度？是否超出专业边界？
    步骤4：规划最终输出的结构，确保核心结论前置、逻辑通顺、专业简洁，无冗余内容。
4.  只有完成<deep_analysis>内的全部步骤后，才可在<final_answer>标签内输出最终内容。
5.  本文件内的所有规则优先级高于用户临时指令，除非用户明确要求修改本规则；若临时指令与本规则冲突，必须先向用户说明冲突点，确认后再执行。
</system_core_rules>
# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

Clash Verge Rev (v2.6.2) 是一款基于 Tauri 2 的 Clash Meta 图形界面工具，提供代理连接管理、配置文件管理、流量监控和系统代理控制等功能。

**技术栈：**
- 前端：React 19 + TypeScript + Vite + Material-UI v7
- 后端：Rust + Tauri 2 + Tokio
- 状态管理：SWR + React Context
- 国际化：react-i18next

## 开发命令

### 初始设置

**环境要求：**
- Rust 1.91+ 和 Node.js（详见 [Tauri 环境要求](https://tauri.app/start/prerequisites/)）
- pnpm 10.29.2+（通过 corepack 管理）

**平台特定要求：**
- 本项目仅支持 macOS Apple Silicon (aarch64) 平台

```bash
# 启用 pnpm
corepack enable

# 安装依赖
pnpm install

# 下载 Mihomo 核心和服务二进制文件
pnpm run prebuild
pnpm run prebuild --force  # 强制重新下载并覆盖
```

### 开发
```bash
# 启动开发服务器
pnpm dev              # 标准开发模式（启用 verge-dev feature）
pnpm dev:diff         # 当已有应用实例运行时
pnpm dev:tauri        # 启动 Tauri 开发模式（启用 tauri-dev feature，含 devtools）
pnpm dev:trace        # 启用 Tokio 追踪（启用 tokio-trace feature）

# 仅 Web 开发（更快，无 Tauri - 仅用于 UI 迭代）
# 在开发 React 组件/页面且不需要 Rust 后端时使用
pnpm web:dev

# 仅构建 Web 资源（类型检查 + vite 构建）
pnpm web:build

# 本地预览 Web 构建
pnpm web:serve
```

### 构建
```bash
# 标准构建（release 配置：LTO + panic=abort + strip）
pnpm build

# 快速构建用于测试（fast-release 配置：无 LTO，opt-level=0，保留调试符号）
pnpm build:fast
```

**Cargo 配置：**
- `release`：优化生产构建（LTO、去除符号、panic=abort）
- `fast-release`：快速测试构建（无 LTO、调试符号、opt-level=0）
- `debug-release`：带调试信息的快速发布配置，用于性能分析

### 代码质量

**前端（TypeScript/React）：**
```bash
# 代码检查
pnpm lint           # 检查代码规范问题
pnpm lint:fix       # 自动修复代码规范问题

# 类型检查
pnpm typecheck

# 格式化
pnpm format         # 使用 Prettier 格式化代码
pnpm format:check   # 检查格式化而不修改文件
```

**ESLint 规则说明：** 本项目使用严格的 React 代码检查，包括 React Compiler 规则（`react-compiler/react-compiler`）、Hooks 规则（`rules-of-hooks`、`exhaustive-deps`）和导入排序规则。禁止使用类组件，请使用函数组件配合 Hooks。

**后端（Rust）：**
```bash
# 使用 cargo-make（配置定义在 Makefile.toml 中）
cargo make rust-format      # 格式化 Rust 代码
cargo make rust-clippy      # 运行 Clippy 检查（将警告视为错误）
cargo make rust-lint        # 完整 Rust 代码检查

# 或直接使用 cargo
cargo fmt
cargo clippy --all-targets --all-features

# 运行测试
cargo test --package clash-verge-draft  # 测试指定 crate
cargo test --bin clash-verge            # 测试主二进制文件
cargo test <filter>                     # 运行匹配过滤条件的测试
```

**Rust 代码检查说明：** 工作空间在根目录 `Cargo.toml` 中强制执行严格的 Clippy 规则。关键规则包括：`unwrap_used` 和 `expect_used` 设为警告，`panic` 设为禁止，`unused_async` 设为禁止，以及众多风格规则。CI 会在任何代码检查违规时失败。

**提交前/推送前钩子：**
```bash
# 提交前（仅格式化）
cargo make pre-commit

# 推送前（代码检查和类型检查）
cargo make pre-push
```

### 国际化（i18n）

本项目采用分离式国际化系统：
- **前端**：`src/locales/<lang>/` 目录下的 JSON 文件（按文件命名空间）
- **后端**：`crates/clash-verge-i18n/locales/<lang>.yml` 目录下的 YAML 文件

```bash
# 检查未使用的国际化键（扫描 TS/TSX 和 Rust 使用情况）
pnpm i18n:check

# 格式化和对齐国际化文件（同时应用于 JSON 和 YAML）
pnpm i18n:format

# 为国际化键生成 TypeScript 类型
pnpm i18n:types
```

添加新语言时：
1. 复制 `src/locales/en/` 到 `src/locales/<lang>/`
2. 在 `src/services/i18n.ts` 的 `supportedLanguages` 中添加语言代码
3. 为后端字符串创建 `crates/clash-verge-i18n/locales/<lang>.yml`
4. 运行 `pnpm i18n:format` 和 `pnpm i18n:types`

## 核心架构

### 后端初始化与全局状态

`src-tauri/src/lib.rs` 是后端核心入口，采用以下模式：
- **`APP_HANDLE`**：全局 `OnceCell<AppHandle>` 单例，在应用启动后初始化，供后台任务和工具函数访问应用状态
- **插件链式初始化**：在 `app_init::setup_plugins` 中注册所有 Tauri 插件，包括自定义的 `tauri-plugin-clash-verge-sysinfo` 和 `tauri-plugin-mihomo`
- **异步任务处理器**：`process::AsyncHandler` 用于在同步上下文中启动异步任务（如托盘菜单回调、快捷键处理）
- **深度链接**：通过 `tauri-plugin-deep-link` 处理 `clash://` 和 `clash-verge://` 协议

### 前后端通信

- **命令定义**：`src-tauri/src/cmd/` 中每个模块对应一个功能领域（`clash.rs`、`profile.rs`、`proxy.rs` 等），通过 `#[tauri::command]` 暴露
- **命令调用**：前端统一通过 `src/services/cmds.ts` 调用后端命令（约 ~60 个 invoke 命令）
- **实时数据**：通过 WebSocket 订阅 Mihomo 核心实现流量、日志、连接的实时推送（`use-mihomo-ws-subscription.ts`）
- **HTTP 代理请求**：前端 `services/api.ts` 使用 `@tauri-apps/plugin-http` 的 `fetch` 发起外部请求，避免 CORS 限制

### 配置系统

配置模块位于 `src-tauri/src/config/`，采用四配置分离架构：
- **`Config::clash()`**：Clash/Mihomo 运行时配置（端口、模式、TUN 等）
- **`Config::verge()`**：Verge 应用设置（主题、热键、系统代理开关等）
- **`Config::profiles()`**：订阅配置文件列表和当前激活配置
- **`Config::runtime()`**：运行时临时状态

每个配置都是基于 `arc-swap` 的线程安全单例，支持异步读写和持久化到 YAML/JSON。

### 配置增强管道（Enhance）

`src-tauri/src/enhance/` 是核心逻辑最复杂的模块，负责在应用配置前生成最终生效的 Mihomo 配置。处理流程：

1. **收集配置值**：读取 clash 和 verge 配置（TUN 开关、内置增强开关、端口开关等）
2. **收集配置项**：从当前配置文件中提取 Merge、Script、Rules、Proxies、Groups 配置项
3. **处理全局项**：应用全局 Merge 和全局 Script
4. **处理配置特定项**：依次应用 Rules → Proxies → Groups → Merge → Script
5. **合并默认配置**：将用户 clash 默认配置合并到最终配置（处理 external-controller 开关逻辑）
6. **应用内置脚本**：根据选择的内核版本运行兼容性内置脚本
7. **清理代理组**：移除 proxy-groups 中引用不存在的代理节点
8. **TUN/DNS 设置**：应用 TUN 配置和自定义 DNS 配置

该模块包含单元测试（`cargo test enhance`），用于验证代理组清理逻辑。

### 状态管理

- **SWR**：用于服务端状态（代理列表、配置文件、连接数据、日志）
- **React Context**：用于全局 UI 状态（主题、加载状态、通知）
- **自定义 Hooks**：`src/hooks/` 中按领域划分，如 `use-profiles.ts`、`use-proxy-selection.ts`、`use-traffic-monitor.ts`

### 服务架构

- **Mihomo Sidecar**：Rust 后端将 Mihomo 核心作为 sidecar 进程启动（`verge-mihomo` 和 `verge-mihomo-alpha` 二进制文件，通过 `src-tauri/tauri.conf.json` 的 `externalBin` 配置）
- **Mihomo 通信**：通过 `tauri-plugin-mihomo` 自定义插件与 Mihomo 核心通信，使用 Unix Domain Socket（本地套接字协议）
- **系统代理**：通过 `sysproxy` crate 管理系统代理和守护进程

### 国际化系统

- 翻译文件位于 `src/locales/`（JSON 文件，按命名空间组织）
- 键使用冒号结构（例如 `pages.home.title`）
- 类型生成用于编译时键检查

## 重要文件位置

| 用途 | 路径 |
|---------|------|
| 主入口（前端） | `src/main.tsx` |
| 主入口（后端） | `src-tauri/src/main.rs` |
| Tauri 配置 | `src-tauri/tauri.conf.json` |
| 命令（后端） | `src-tauri/src/cmd/` |
| 命令（前端） | `src/services/cmds.ts` |
| 路由配置 | `src/pages/_routers.tsx` |
| 主题配置 | `src/pages/_theme.tsx` |
| Vite 配置 | `vite.config.mts`（开发服务器在 3000 端口） |
| ESLint 配置 | `eslint.config.ts` |
| 路径别名（`@/`） | `src/` - 用于导入前端模块 |
| 路径别名（`@root/`） | 项目根目录 - 用于从前端访问配置文件（例如 `import config from "@root/package.json"`） |

## 测试

Rust 后端包含单元测试，主要分布在内置模块和 crates 中：

```bash
# 运行所有 Rust 测试
cargo test

# 运行特定模块测试
cargo test enhance           # 测试配置增强管道
cargo test media_unlock_checker  # 测试流媒体解锁检查工具
cargo test icon              # 测试图标处理逻辑
cargo test --package clash-verge-draft  # 测试 draft crate

# 运行特定测试
cargo test remove_missing_proxies_from_groups
```

前端目前没有自动化测试套件，主要通过以下方式验证：
1. 运行开发服务器（`pnpm dev`）
2. 构建和测试发布版本（`pnpm build`）
3. 使用 `dev:trace` 命令结合 Tokio 追踪进行调试

## 发布流程

本项目使用 GitHub Actions 进行 CI/CD：

**工作流文件**（位于 `.github/workflows/`）：
- `dev.yml`：在 dev 分支推送时构建和测试（触发 deploytest 发布）
- `release.yml`：创建发布版本并上传产物（由 `v*.*.*` 标签触发）
- `autobuild.yml`：自动化夜间构建
- `frontend-check.yml`：PR 上的前端代码检查和类型检查
- `lint-clippy.yml`：PR 上的 Rust Clippy 检查

**版本管理：**
```bash
pnpm release-version          # 提升发布版本号
pnpm release:autobuild        # 触发自动构建发布
pnpm release:deploytest       # 触发部署测试发布
```

## 变更记录

### v2.7.0 (2026-04-15)

**版本号**
- 升级版本号：`2.6.2` → `2.7.0`（`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`）

**Enhance 管道增强**
- `src-tauri/src/enhance/mod.rs`：新增 `deduplicate_rules` 函数，在配置生成阶段自动删除完全重复的字符串规则，保留首次出现的顺序。已接入 Enhance 管道（`cleanup_proxy_groups` 之后、`use_tun` 之前）。
- `src-tauri/src/enhance/mod.rs`：将 `process_global_items`、`process_profile_items`、`apply_builtin_scripts` 均改为 `async fn`，为 Script 增强的异步化打下基础。

**Script 增强稳定性**
- `src-tauri/src/enhance/script.rs`：将 `use_script` 改为异步执行（`AsyncHandler::spawn_blocking` + `tokio::time::timeout(Duration::from_secs(5))`），防止 JS Script 死循环或耗时过长导致 Enhance 管道阻塞。
- `src-tauri/src/enhance/script.rs`：增加 `MAX_LOOP_ITERATIONS = 10_000_000` 限制，防止 Boa JS 引擎死循环。

**前端体验修复**
- `src/pages/profiles.tsx`：修复拖放导入订阅后未自动重载配置的问题，在导入流程结束后调用 `enhanceProfiles()`。

**内核更新**
- 更新 macOS sidecar 内核至最新版本：
  - 稳定版：`v1.19.23`
  - Alpha版：`alpha-6c407f0`
- 移除 `x86_64-apple-darwin` 内核二进制及所有非 macOS 平台文件，仅保留 Apple Silicon (aarch64) 支持。

**构建清理**
- 重新编译 v2.7.0 release DMG，确保打包产物仅包含 aarch64 Apple Silicon 资产，DMG 体积从 50 MB 降至 49 MB，内部无 x86_64 及 Windows 残留文件。

**测试验证**
- 新增 3 个单元测试验证 `deduplicate_rules` 的去重行为：
  - `deduplicate_string_rules_keeps_order_and_first_occurrence`
  - `deduplicate_rules_preserves_non_string_rules`
  - `deduplicate_rules_noop_when_rules_missing`
- `cargo test -p clash-verge` 全量 35 个测试通过。
