# OpenCode Go 润色接入

在现有语音识别完成后，使用内置 OpenCode Go 服务商对识别文字进行润色。

## 使用

1. 编译并启动修改后的应用，在设置中的 AI 润色服务商选择 **OpenCode Go**。
2. 在应用内填写 Go 订阅对应的 API Key。密钥沿用系统密钥环存储，不写入源码。
3. 默认模型为 `glm-5.2`；可选择预设模型、刷新模型列表或填写模型 ID。
   直接 API 使用 `glm-5.2` 这样的 ID，不加 `opencode-go/` 前缀。
4. 开启 AI 润色，保留当前语音识别引擎，录音测试。
5. 若接口拒绝请求或超时，现有听写流程提示错误并保留原始识别文字。

Go 是面向编程代理的订阅。普通听写润色不保证被服务端接受；
客户端标识和会话 ID 用于正确标识请求，不用于伪装编程代理。
本次自动化验证使用本地模拟接口，不代表已验证真实订阅或额度。
官方说明：https://opencode.ai/docs/go/#where-can-i-use-it

## 接口

固定 Base URL：`https://opencode.ai/zen/go/v1`。
模型列表：`/models`，API Key 独立于其他服务商保存。

- GLM、Kimi、DeepSeek 等：`/chat/completions`
- GPT 5.6 Luna、Grok 4.6、已列出的 Muse Spark：`/responses`
- 已列出的 MiniMax、Qwen 模型：`/messages`

映射依据官方接口表：https://opencode.ai/docs/go/#endpoints
新模型未列入映射时默认使用 Chat Completions；如新模型需要其他协议，
可用现有自定义服务商指定完整 endpoint 和协议，或更新映射。
只有内置 OpenCode Go 服务商自动附加 Go 会话请求头。

## 开发验证

```powershell
pnpm install --frozen-lockfile
pnpm check
cargo test --manifest-path "src-tauri/Cargo.toml" --lib
cargo fmt --manifest-path "src-tauri/Cargo.toml" -- --check
pnpm tauri dev
```

发布安装包仍按仓库 README 准备真实 Python 引擎归档后构建。
