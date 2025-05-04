# 模板表达式语法

本说明文档定义了模板表达式语法，用于配置文件中的动态字符串生成。

快速查看 - [内置函数列表](#内置函数列表)

---

## 基本格式

表达式使用 `${…}` 包裹：

```text
${<函数名|变量名>[(:<定义名>)][:<参数1>[,<参数2>,…]]}
```

- `<函数名|变量名>`：必填，支持函数调用或变量引用
- `(:<定义名>)`：可选，声明命名变量（如 `${username(:user)}`）
- `:<参数列表>`：可选，多参数用逗号 `,` 分隔
- 如果函数不需要参数，可写成 `${fn}` 或 `${fn:}`
- 变量使用示例
  ```toml
  params = { "password" = "${password(:pass)}", "nextpassword" = "${pass}" }
  ```

---

## 支持的参数类型

1. **字符串字面量**（双引号）

   - 内部 `"` 和 `\` 用 `\"`、`\\` 转义
   - 例：

     ```text
     ${base64:"Hello, world!"}
     ${replace:"C:\\path\\file.txt","\\","/"}
     ```

2. **数字字面量**

   - 例：`${substr:"abcdef",2,3}`

3. **嵌套函数调用**

   - 例：`${upper:${username}}`

4. **反引号模板字符串**

   - 支持在参数中使用 `` `${…}` `` 嵌套表达式
   - 例：

     ```text
     ${base64:`ID:${qqid}, user:${username}`}
     ```

     - 先渲染 `` `User=${username};ID=${qqid}` `` 结果
     - 再对整体做 Base64 编码

---

## 内置函数列表

| 函数       | 参数                     | 说明         | 示例                                              |
| ---------- | ------------------------ | ------------ | ------------------------------------------------- |
| `username` | —                        | 随机用户名   | `${username}`                                     |
| `password` | —                        | 随机密码     | `${password}`                                     |
| `qqid`     | —                        | 随机 QQ 号   | `${qqid}`                                         |
| `email`    | —                        | 随机电子邮箱 | `${email}`                                        |
| `base64`   | `string`                 | Base64 编码  | `${base64:"test"}` → `dGVzdA==`                   |
| `upper`    | `string`                 | 转大写       | `${upper:"hello"}` → `HELLO`                      |
| `lower`    | `string`                 | 转小写       | `${lower:"HELLO"}` → `hello`                      |
| `replace`  | `str`, `old`, `new`      | 全部替换     | `${replace:"a.b.c",".","-"}` → `a-b-c`            |
| `substr`   | `str`, `start`\[, `len`] | 取子串       | `${substr:"abcdef",1,3}` → `bcd`                  |
| `random`   | `type`, …                | 生成随机值   | `${random:chars,8}` <br> `${random:number,1,100}` |

---

### `random` 模式详解

- **`random:chars,length[,charset]`**
  生成随机字符串（默认字符集 A–Z/a–z/0–9）。
  例：`${random:chars,5}`、`${random:chars,4,"abc"}`

- **`random:number,max`**
  生成 `0` 到 `max` 的整数（含）。
  例：`${random:number,10}`

- **`random:number,min,max`**
  生成 `min` 到 `max` 的整数（含）。
  例：`${random:number,100,200}`

---

## 嵌套与组合示例

```text
${base64:${replace:${username},"@","_"}}
```

解析顺序：

1. `${username}` → `Steve123`
2. `${replace:"alice@example.com","@","_"}` → `alice_example.com`
3. `${base64:"alice_example.com"}` → `YWxpY2VfZXhhbXBsZS5jb20=`

---
