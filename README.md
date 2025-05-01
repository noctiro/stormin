# Stormin

Stormin 是一个注册轰炸机，专门用于整治盗号网站为生。

支持大量生成虚假账号/QQ号和密码来攻击骗子的网站。

这个是一个练习项目，欢迎各位大佬的指导和PR。

## 配置文件说明

Stormin 使用 TOML 格式的配置文件来定义请求目标和参数。配置文件名为 `config.toml` ，可以参考项目的默认配置文件 `example.config.toml`。

### 基本结构

```toml
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

在`params`中可以使用以下特殊语法：

1. 变量引用：使用`${变量名}`语法

   - `${user}` - 随机用户名
   - `${password}` - 随机密码
   - `${qqid}` - 随机 QQ 号

2. 函数调用：使用`${函数名:参数}`语法

   - `${base64:文本}` - Base64 编码

3. 支持嵌套调用：
   ```toml
   params = { sv = "${base64:${base64:${password}}}" }
   ```

### 参数说明

1. 顶级配置

   - `proxy_file`: 代理列表文件路径（可选）

2. Target 配置

   - `url`: 必填，目标 URL
   - `method`: 可选，HTTP 请求方法，默认为"GET"
   - `headers`: 可选，自定义 HTTP 请求头
   - `params`: 可选，URL 查询参数

3. 内置变量

   - `${user}`: 生成随机用户名
   - `${password}`: 生成随机密码（包括常见社工密码模式和完全随机密码）
   - `${qqid}`: 生成随机 QQ 号

4. 内置函数
   - `base64`: Base64 编码，支持嵌套调用

### 注意事项

1. 所有的变量和函数调用都使用`${}`语法
2. 函数调用支持嵌套，如`${base64:${base64:值}}`
3. 参数值中如果包含特殊字符，可以使用 TOML 的字符串语法：
   ```toml
   params = {
       complex = """${base64:复杂的${password}值}"""
   }
   ```

### TUI 快捷键

- `P`: 暂停
- `R`: 恢复
- `Q`: 退出
