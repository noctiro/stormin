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
threads = 16                       # 线程数 推荐 CPU 核数 * 4
proxy_file = "proxies.txt"         # 代理文件路径（可选）
timeout = 5                        # 超时时间，单位秒

# 动态速率控制参数 (均为可选)
# target_rps = 100.0                 # 目标每秒请求数 (RPS)。如果未设置，将使用内部的自适应延迟逻辑。
# min_success_rate = 0.95            # 最小发送成功率 (0.0 到 1.0)。
# rps_adjust_factor = 0.2            # RPS 调整因子 (若不设置，代码中默认为 0.2)。控制向 target_rps 调整的速度。
# success_rate_penalty_factor = 1.5  # 成功率惩罚因子 (若不设置，代码中默认为 1.5)。低于 min_success_rate 时应用。

[[Target]]                  # 定义第一个目标
url = "http://example.com"  # 目标URL
method = "POST"             # HTTP方法（可选，默认为GET）
headers = { }               # 自定义请求头(可以使用模板语法)（可选）
params = { }                # URL参数(可以使用模板语法)（可选）

[[Target]]                  # 可以定义多个目标
# ... 其他目标配置
```

### 动态速率控制说明

- `target_rps`: (可选) 设置数据生成器尝试达到的目标每秒请求数。如果未设置，将使用基本的自适应延迟。
- `min_success_rate`: (可选) 设置一个介于 0.0 和 1.0 之间的值。如果数据发送的成功率（基于最近的发送尝试）低于此阈值，数据生成器将增加延迟以尝试提高成功率。
- `rps_adjust_factor`: (可选, 默认为 0.2) 控制当设置了 `target_rps` 时，数据生成器向目标延迟调整的速度。较小的值调整较慢，较大的值调整较快。
- `success_rate_penalty_factor`: (可选, 默认为 1.5) 当实际发送成功率低于 `min_success_rate` 时，当前的发送延迟会乘以这个因子。

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

## 免责声明

本项目仅用于技术学习与安全研究目的，**严禁用于非法用途**。使用本工具造成的任何后果由使用者自行承担。
