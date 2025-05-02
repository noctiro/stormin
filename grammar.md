# 模板表达式语法

本说明文档定义了模板表达式的语法，用于在配置文件等场景中进行动态字符串生成。

快速查看 - [内置函数列表](#内置函数)

---

## 基本格式

表达式使用 `${...}` 包裹：

```text
${<function_name>[:<arg1>[,<arg2>,...]]}
```

- `<function_name>`：函数名
- `:<args>`：可选，用于函数调用。多个参数用逗号 `,` 分隔

---

## 函数调用

函数使用 `函数名:参数列表` 的形式调用。如果函数不需要参数，可以省略冒号和参数列表 (`${function_name}` 或 `${function_name:}` 效果相同)。

### 参数类型支持

1. **字符串字面量**（需用双引号包裹）：

   - 内部的 `"` 和 `\` 需要转义为 `\"` 和 `\\`
   - 示例：

     ```text
     ${base64:"Hello, world!"}
     ${replace:"File \"C:\\path\"","\\","/"}
     ```

2. **函数嵌套**：

   - 函数可以作为其他函数的参数使用
   - 示例：

     ```text
     ${upper:${user}}
     ${base64:${upper:${user}}}
     ```

---

## 内置函数

以下是当前支持的函数（实现见 `src/template.rs`）：

| 函数名     | 参数                        | 说明                                                           | 示例                                               |
| ---------- | --------------------------- | -------------------------------------------------------------- | -------------------------------------------------- |
| `username` | (无)                        | 随机用户名名                                                   | `${username}`                                      |
| `password` | (无)                        | 随机密码码                                                     | `${password}`                                      |
| `qqid`     | (无)                        | 生成随机 QQ 号                                                 | `${qqid}`                                          |
| `base64`   | `string`                    | 将输入字符串进行 Base64 编码                                   | `${base64:"test"}` → `dGVzdA==`                    |
| `upper`    | `string`                    | 将字符串转为大写                                               | `${upper:"hello"}` → `HELLO`                       |
| `lower`    | `string`                    | 将字符串转为小写                                               | `${lower:"HELLO"}` → `hello`                       |
| `replace`  | `str`, `old`, `new`         | 将 `str` 中的所有 `old` 替换为 `new`                           | `${replace:${user},"@","_"}`                       |
| `substr`   | `str`, `start`\[, `length`] | 提取从 `start` 开始（从 0 计数）的子串，可选 `length` 限制长度 | `${substr:${password},0,4}`                        |
| `random`   | `type`, ...                 | 生成随机数据（详见下方）                                       | `${random:chars,10}` <br> `${random:number,1,100}` |

---

### `random` 函数详解

随机数据生成支持以下模式：

1. **`random:chars,length[,charset]`**

   - 生成指定长度的随机字符串
   - 可指定自定义字符集（默认为 `A-Za-z0-9`）
   - 示例：

     ```text
     ${random:chars,8}          // 例如：aB3x9ZpQ
     ${random:chars,5,"abc"}    // 例如：bacca
     ```

2. **`random:number,max`**

   - 生成 `0 ~ max` 的随机整数（包含 max）
   - 示例：

     ```text
     ${random:number,10}        // 例如：7
     ```

3. **`random:number,min,max`**

   - 生成 `min ~ max` 的随机整数（包含 min 与 max）
   - 示例：

     ```text
     ${random:number,100,200}   // 例如：153
     ```

---

## 嵌套示例：组合多步操作

```text
${base64:${replace:${user},"@","_"}}
```

解析顺序：

1. `${user}` → 例如 `test@example.com`
2. `${replace:"test@example.com","@","_"}` → `test_example.com`
3. `${base64:"test_example.com"}` → `dGVzdF9leGFtcGxlLmNvbQ==`

---
