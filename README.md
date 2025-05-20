# Stormin

Stormin 是一个拥有 TUI 界面的注册轰炸机，专门用于整治盗号网站。

通过内置的模板语法，支持生成大量的各类虚假账号信息来进行轰炸。

这个是一个练习项目，欢迎各位大佬的指导和 PR。

盗号网站贡献与配置分享 -> [Discussions #1](https://github.com/Noctiro/stormin/discussions/1)

## 配置文件说明

默认配置文件为同级文件夹下的 `config.toml` 文件，你可以使用 `--config=filename` 来使用其他名字的配置文件。

你可以浏览文档和参考项目中的 `example.config.toml` 来学习如何编写配置文件。

### 基本结构

```toml
threads = 64                       # 线程数 (可选，默认为 CPU 核数 * 16 )
generator_threads = 1              # 生成线程数，一般1就够用 (可选)
proxy_file = "proxies.txt"         # 代理文件路径（可选）
timeout = 5                        # 超时时间，单位秒 (可选，默认 5 秒)
cli_update_interval_secs = 1       # CLI模式下的统计信息更新间隔（秒）（可选）
start_paused = false               # 是否以暂停状态启动（可选，默认 false）
run_duration = "30m"               # 运行持续时间（可选，如 "10s", "5m", "1h"）

# --- 数据生成速率配置 (所有参数可选) ---
min_delay_micros = 1000          # 最小生成延迟，默认1000微秒(1ms)
max_delay_micros = 100000        # 最大生成延迟，默认100000微秒(100ms)
initial_delay_micros = 5000      # 初始生成延迟，默认5000微秒(5ms)
increase_factor = 1.2            # 延迟增加因子，默认1.2(每次增加20%)
decrease_factor = 0.85           # 延迟减少因子，默认0.85(每次减少15%)

[[Target]]                  # 定义第一个目标
url = "http://example.com"  # 目标URL
method = "POST"             # HTTP方法（可选，默认为GET）
headers = { }               # 自定义请求头(可以使用模板语法)（可选）
params = { }                # URL参数(可以使用模板语法)（可选）

[[Target]]                  # 可以定义多个目标
# ... 其他目标配置
```

### 数据生成速率说明

程序使用自适应的速率控制系统，通过动态调整生成延迟来平衡性能和资源使用：

- `min_delay_micros`: 最小生成延迟（微秒）。默认 1000 微秒（1 毫秒），防止生成过快消耗过多资源。
- `max_delay_micros`: 最大生成延迟（微秒）。默认 100000 微秒（100 毫秒），避免生成过慢影响性能。
- `initial_delay_micros`: 初始生成延迟（微秒）。默认 5000 微秒（5 毫秒），启动时的基准延迟。
- `increase_factor`: 当数据池满时，延迟增加的系数。默认 1.2，表示每次增加 20%延迟。
- `decrease_factor`: 当数据发送成功时，延迟减少的系数。默认 0.85，表示每次减少 15%延迟。

### CLI 模式配置说明

- `cli_update_interval_secs`: 在 CLI 模式下，统计信息的更新间隔时间（秒）。
- `start_paused`: 是否以暂停状态启动程序。默认为 false，即程序启动后立即开始执行。
- `run_duration`: 程序的运行时长。支持秒(s)、分钟(m)、小时(h)的组合，如 "30s"、"5m"、"1h30m"。不设置则持续运行直到手动停止。

### 参数模板语法

可以达到各种各样的效果，如

```toml
params = { sv = "${base64:${base64:${password}}}" }
```

详情请见 [模板表达式语法](grammar.md)

### TUI 快捷键

- `P`: 暂停
- `R`: 恢复
- `Q`: 退出

### CLI 模式

通过 `--cli` 参数启用，默认为 TUI 模式

## 免责声明

本项目仅用于技术学习与安全研究目的，**严禁用于非法用途**。使用本工具造成的任何后果由使用者自行承担。
