# 日志保存

应用日志和内核日志统一归档到应用数据目录下的 `logs/v{VERSION}/`。macOS 应用数据目录为 `~/Library/Application Support/io.github.clash-verge-rev.clash-verge-rev/`。

| 日志类型 | 当前文件 |
| --- | --- |
| 应用运行日志 | `logs/v{VERSION}/latest.log` |
| Sidecar 内核日志 | `logs/v{VERSION}/sidecar/sidecar_latest.log` |
| Service 内核日志 | `logs/v{VERSION}/service/service_latest.log` |

托盘菜单的「内核日志」按当前运行模式打开对应文件。各类日志沿用应用设置中的文件大小上限和保留数量，旧文件在对应目录内轮转。

Service 内核日志由 Rust 后台独立订阅，包含 debug、info、warning 和 error 级别。关闭窗口、进入轻量模式，或在日志页面执行暂停、清除和筛选，均不停止文件记录。应用日志级别只影响应用运行日志。

内核启动或连接中断后，后台连接实时日志流，并通过已认证的 Service IPC 补充服务保留的最近 100 条输出。补充记录带有内核原始时间，可能与已保存的实时记录重叠；超出服务缓存的断线期间记录无法保证补齐。日志页面自身的 1,000 条缓存不作为文件保存来源。

特权服务仍保留系统目录中的受保护日志副本，供故障恢复使用。应用不要求服务以管理员权限写入用户提供的任意路径，也不修改该目录的访问权限。此改动不迁移系统目录中已经轮转的历史日志；需运行包含此改动的应用版本后，后台归档才会生效。
