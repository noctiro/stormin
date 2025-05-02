# Stormin

Stormin 是一个注册轰炸机，专门用于整治盗号网站为生。

通过内置的模板语法，支持生成大量的各类虚假账号信息来进行轰炸。

这个是一个练习项目，欢迎各位大佬的指导和 PR。

## 配置文件说明

Stormin 使用 TOML 格式的配置文件来定义请求目标和参数。配置文件名为 `config.toml` ，可以参考项目的默认配置文件 `example.config.toml`。

### 基本结构

```toml
threads = 16                # 线程数 推荐 CPU 核数 * 4
proxy_file = "proxies.txt"  # 代理文件路径（可选）

[[Target]]                  # 定义第一个目标
url = "http://example.com"  # 目标URL
method = "POST"             # HTTP方法（可选，默认为GET）
headers = { }               # 自定义请求头（可选）
params = { }                # URL参数（可选）

[[Target]]                  # 可以定义多个目标
# ... 其他目标配置
```

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
