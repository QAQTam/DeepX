# Provider Switch — 多模型提供商适配点分析

## 当前架构

```
Config.provider_id + Config.endpoint
  → ProviderConfig { base_url, api_key, model, user_id_mode }
    → openai.rs 构建请求 URL + body + 解析响应
```

**问题：** `ProviderConfig` 只传递 `base_url`，不传递 `EndpointSpec` 中的差异化配置。gate 只能用 OpenA/DeepSeek 的约定。

---

## 适配点清单

### 1. Chat URL 路径 — ⚠️ Qwen 不兼容

| Provider | 当前行为 | 要求 |
|----------|---------|------|
| DeepSeek | `{base}/chat/completions` ✓ | 无 /v1 |
| Qwen | `{base}/chat/completions` ✗ | `/compatible-mode/v1/chat/completions` |
| Anthropic | 不支持 | `/v1/messages` (完全不同的 API) |

**方案：** `EndpointSpec` 加 `chat_path: Option<String>`，None 时用默认 `"/chat/completions"`。

**涉及文件：**
- `deepx-types/src/provider.rs` — EndpointSpec 加字段
- `deepx-config/src/registry.rs` — 注册时填写
- `deepx-gate/src/types.rs` — ProviderConfig 传递 chat_path
- `deepx-gate/src/openai.rs` — build_chat_url 读取 chat_path

---

### 2. 思考模式参数格式 — ⚠️ Qwen 不兼容

| Provider | 当前参数格式 | 要求 |
|----------|------------|------|
| DeepSeek | `"thinking": {"type": "enabled"}` (top-level) | 标准 OpenAI thinking |
| Qwen | `"thinking": {"type": "enabled"}` ✗ | `"extra_body": {"enable_thinking": true}` |

**Qwen 的 `enable_thinking` 不在标准 OpenAI body 中，需通过 extra_body 传递。**
且 Qwen 的 `reasoning_effort` 也在 extra_body 中：`"reasoning_effort": "high"`（仅限 deepseek-v4 系列，且要求 extra_body）。

**方案：** `EndpointSpec` 加 `thinking_mode: ThinkingMode` 枚举：
```rust
pub enum ThinkingMode {
    OpenAi,        // thinking: {type: "enabled"} (top-level, 默认)
    QwenExtraBody, // enable_thinking: true (extra_body)
}
```

**涉及文件：**
- `deepx-types/src/provider.rs` — ThinkingMode 枚举 + EndpointSpec 加字段
- `deepx-gate/src/openai.rs` — 请求体构建按 thinking_mode 分支

---

### 3. 缓存命中 Token 字段 — ⚠️ Qwen 不兼容

| Provider | 当前读取路径 | 实际路径 |
|----------|-----------|---------|
| DeepSeek | `usage.prompt_cache_hit_tokens` (top-level) | 同 |
| Qwen | `usage.prompt_cache_hit_tokens` ✗ | `usage.prompt_tokens_details.cached_tokens` |

**方案：** `EndpointSpec` 加 `cache_field: CacheTokenField` 枚举：
```rust
pub enum CacheTokenField {
    PromptCacheHitTokens,  // 默认: usage.prompt_cache_hit_tokens
    CacheTokens,           // Qwen: usage.prompt_tokens_details.cached_tokens
    None,                  // 不支持缓存
}
```

**涉及文件：**
- `deepx-types/src/provider.rs` — CacheTokenField 枚举 + EndpointSpec 加字段
- `deepx-gate/src/openai.rs` — usage 解析按 cache_field 分支

**注意：** Qwen 的 prompt_cache_miss_tokens 也没有对应字段（只有 cached_tokens）。miss = prompt_tokens - cached_tokens（可推算）。

---

### 4. Balance 端点 — Qwen 无此端点

| Provider | 当前行为 | 预期 |
|----------|---------|------|
| DeepSeek | GET `{base}/user/balance` ✓ | 正常返回 |
| Qwen | GET `{base}/user/balance` ✗ | 返回 404 或错误 |

**方案：** `EndpointSpec` 加 `has_balance: bool`（默认 true），false 时跳过查询。

**涉及文件：**
- `deepx-types/src/provider.rs` — EndpointSpec 加字段
- `deepx-gate/src/openai.rs` — query_balance 条件跳过

---

### 5. Models 列表端点路径

| Provider | 当前行为 | 要求 |
|----------|---------|------|
| DeepSeek | GET `{models_url}/models` ✓ | 同 |
| Qwen | GET `{base}/models` ？ | 待确认 |

**方案：** `models_url` 字段已存在，Qwen 可直接填完整路径（如 `https://dashscope.aliyuncs.com/compatible-mode/v1`）。不需要改动。

---

### 6. SS SSE 增量字段 — 待验证

| Delta 字段 | DeepSeek | Qwen |
|-----------|---------|------|
| `delta.content` | ✓ | ✓ |
| `delta.reasoning_content` | ✓ | 待确认（Qwen 在思考模式下应该是同字段） |
| `delta.tool_calls[].function.arguments` | ✓ | 待确认 |

当前代码已在 `openai.rs:194,227,234` 处理这三个 delta 路径。如果 Qwen 返回相同结构，无需改动。

---

### 7. user_id / user_id_mode

| Provider | 行为 |
|----------|------|
| DeepSeek | `user_id_mode: Some(Body)` → 请求体注入 `user_id`（session seed 用于 KV cache 亲和） |
| Qwen | 待确认是否支持 |

不影响功能，可暂时不动。

---

## 改动文件汇总

| 文件 | 改动类型 | 大小 |
|------|---------|------|
| `deepx-types/src/provider.rs` | 新增 2-3 个枚举 + EndpointSpec 加 4 个字段 | ~20 行 |
| `deepx-config/src/registry.rs` | 注册 Qwen provider | ~20 行 |
| `deepx-gate/src/types.rs` | ProviderConfig 传递 EndpointSpec 字段（或直接传 EndpointSpec） | ~10 行 |
| `deepx-gate/src/openai.rs` | chat_path 分支 + thinking_mode 分支 + cache_field 分支 + balance 跳过 | ~30 行 |
| `deepx-msglp/src/lib.rs` | 构造 ProviderConfig 时传入 EndpointSpec | ~5 行 |

**总计约 85 行 Rust。**

---

## 待分析 Provider

- [x] ~~GLM (智谱AI)~~ — 仅 2 项差异
- [x] ~~Kimi (月之暗面)~~ — 仅 3 项差异
- [x] ~~MiMo (小米)~~ — 仅 2 项差异
- [x] ~~MiniMax (稀宇)~~ — 4 项差异，见下方
- [x] ~~Doubao/火山方舟 (字节)~~ — 仅 1 项差异，见下方
- [ ] OpenAI (官方) — 标准 OpenAI，基准已覆盖
- [ ] Anthropic Claude — 完全不同的 API 协议（非本文档范围，需 gate 新增 `claude.rs`）
- [ ] Google Gemini — 同 Anthropic，独立协议
- [ ] Groq / Together / Fireworks — OpenAI 兼容，预计不需要额外改动
- [ ] 硅基流动 (SiliconFlow) — OpenAI 兼容，验证 thinking 参数格式

---

## GLM (智谱AI) 适配分析

**基础信息：**
- Base URL: `https://open.bigmodel.cn/api/paas/v4`
- Chat: `{base}/chat/completions`
- 文档: https://docs.bigmodel.cn/cn/api/introduction

### 适配点差异矩阵

| 适配点 | DeepSeek (当前) | Qwen (千问) | GLM (智谱) |
|--------|----------------|-------------|------------|
| **Chat URL 前缀** | 无前缀 | `/compatible-mode/v1` | `/api/paas/v4` |
| **Thinking 参数** | `"thinking": {"type": "enabled"}` (top-level) | `"enable_thinking": true` (extra\_body) | `"thinking": {"type": "enabled"}` (top-level) ✓ |
| **Cache 字段** | `usage.prompt_cache_hit_tokens` | `usage.prompt_tokens_details.cached_tokens` | `usage.prompt_tokens_details.cached_tokens` |
| **Reasoning 内容** | `delta.reasoning_content` | 待确认 | `delta.reasoning_content` ✓ |
| **Tool calls** | 标准 OpenAI | 标准 OpenAI | 标准 OpenAI ✓ |
| **Balance 端点** | `/user/balance` | 不支持 | 待确认（大概不支持） |
| **user\_id** | Body 注入 | 待确认 | 待确认 |

### 关键发现

GLM 的 `extra_body` 是 OpenAI Python SDK 的包装概念，实际 HTTP 请求中 `thinking` 在顶层 body。
**Thinking 参数与 DeepSeek 零差异。**

仅 2 项需要适配：
1. URL 前缀 `/api/paas/v4` — `chat_path` 字段已覆盖
2. 缓存字段 `prompt_tokens_details.cached_tokens` — `cache_field` 枚举已覆盖

### ThinkingMode 枚举最终设计

```rust
pub enum ThinkingParamMode {
    OpenAi,             // 默认: thinking: {type: "enabled"} (top-level, DeepSeek + GLM)
    QwenEnableThinking, // Qwen: enable_thinking: true (top-level, Qwen 专用)
}
```

### 更新后的改动文件汇总

| 文件 | 改动 | 行数 |
|------|------|------|
| `deepx-types/src/provider.rs` | 新增 ThinkingParamMode + CacheTokenField 枚举 + EndpointSpec 加 4 字段 | ~25 |
| `deepx-config/src/registry.rs` | 注册 Qwen + GLM provider | ~40 |
| `deepx-gate/src/types.rs` | ProviderConfig 传递 4 个适配字段 | ~15 |
| `deepx-gate/src/openai.rs` | chat_path/thinking/cache/balance 分支 | ~35 |
| `deepx-msglp/src/lib.rs` | 构造 ProviderConfig 时查 EndpointSpec | ~5 |

**总计约 120 行 Rust，覆盖 DeepSeek + Qwen + GLM。**

---

## Kimi (月之暗面) 适配分析

**基础信息：**
- Base URL: `https://api.moonshot.cn/v1`
- Chat: `{base}/chat/completions`
- Balance: `{base}/users/me/balance`
- Models: `{base}/models`
- 文档: https://platform.kimi.com/docs/api/overview

### 适配点差异矩阵

| 适配点 | DeepSeek (当前) | Qwen (千问) | GLM (智谱) | Kimi (月之暗面) |
|--------|----------------|-------------|------------|----------------|
| **Chat URL 前缀** | 无前缀 | `/compatible-mode/v1` | `/api/paas/v4` | `/v1` (标准) |
| **Thinking 参数** | `thinking: {type: "enabled"}` | `enable_thinking: true` | `thinking: {type: "enabled"}` ✓ | `thinking: {type: "enabled"}` ✓ |
| **Cache 字段** | `usage.prompt_cache_hit_tokens` | `usage.prompt_tokens_details.cached_tokens` (嵌套) | `usage.prompt_tokens_details.cached_tokens` (嵌套) | `usage.cached_tokens` (顶层，不同名) |
| **Balance 路径** | `/user/balance` | 不支持 | 不支持 | `/users/me/balance` (不同路径) |
| **Reasoning** | `delta.reasoning_content` | 待确认 | `delta.reasoning_content` ✓ | `delta.reasoning_content` ✓ |
| **Tool calls** | 标准 OpenAI | 标准 OpenAI | 标准 OpenAI | 标准 OpenAI ✓ |
| **user\_id** | Body 注入 | 待确认 | 待确认 | `prompt_cache_key` (不同字段名) |
| **非思考模型** | N/A | N/A | N/A | moonshot-v1-* 不支持 thinking，需按模型禁用 |

### 新增差异点

1. **缓存字段名不同** — Kimi 是 `usage.cached_tokens`（顶层），既不是 DeepSeek 的 `prompt_cache_hit_tokens`，也不是 Qwen/GLM 的嵌套路径。需要第三种变体。

2. **Balance 路径不同** — Kimi 是 `/users/me/balance`，DeepSeek 是 `/user/balance`。需要 `balance_path` 字段。

3. **非思考模型** — Kimi 的 `moonshot-v1-*` 系列不支持 `thinking` 参数，必须在请求体中省略。需 `supports_thinking` 标志。

4. **`prompt_cache_key`** — Kimi 提供此字段用于 KV cache 亲和（类似 DeepSeek 的 `user_id` 但字段名不同）。

### CacheTokenField 枚举最终版

```rust
pub enum CacheTokenField {
    PromptCacheHitTokens,   // DeepSeek: usage.prompt_cache_hit_tokens + prompt_cache_miss_tokens
    PromptDetailsCached,    // Qwen/GLM: usage.prompt_tokens_details.cached_tokens
    UsageCachedTokens,      // Kimi: usage.cached_tokens (top-level, single value)
}
```

### EndpointSpec 最终字段清单

```rust
pub struct EndpointSpec {
    // 现有字段
    pub id: String,
    pub display: String,
    pub protocol: String,
    pub base_url: String,
    pub default_model: String,
    pub models: Vec<String>,
    pub models_url: Option<String>,
    pub user_id_mode: Option<UserSendMode>,

    // 新增适配字段
    pub chat_path: Option<String>,               // None → "/chat/completions"
    pub balance_path: Option<String>,            // None → "/user/balance"
    pub thinking_mode: ThinkingParamMode,        // 默认 OpenAi
    pub cache_field: CacheTokenField,            // 默认 PromptCacheHitTokens
    pub has_balance: bool,                       // 默认 true
    pub supports_thinking: bool,                 // 默认 true (false for moonshot-v1)
}
```

### Kimi 注册示例

```rust
fn kimi() -> ProviderSpec {
    ProviderSpec {
        id: "kimi".into(),
        display: "Kimi".into(),
        endpoints: vec![
            EndpointSpec {
                id: "kimi-k2".into(),
                display: "Kimi K2 (Thinking)".into(),
                base_url: "https://api.moonshot.cn/v1".into(),
                default_model: "kimi-k2.6".into(),
                models: vec!["kimi-k2.6".into(), "kimi-k2.5".into()],
                // chat_path: None → 用默认 "/chat/completions" ✓
                // thinking_mode: OpenAi → 标准 thinking 格式 ✓
                balance_path: Some("/users/me/balance".into()),
                cache_field: CacheTokenField::UsageCachedTokens,
                supports_thinking: true,
                // ...
            },
            EndpointSpec {
                id: "moonshot-v1".into(),
                display: "Moonshot V1 (No Thinking)".into(),
                base_url: "https://api.moonshot.cn/v1".into(),
                default_model: "moonshot-v1-auto".into(),
                models: vec!["moonshot-v1-auto".into(), "moonshot-v1-8k".into(), "moonshot-v1-32k".into(), "moonshot-v1-128k".into()],
                supports_thinking: false,  // moonshot-v1 不接受 thinking 参数
                // ...
            },
        ],
    }
}
```

### 更新后的汇总

| 文件 | 改动 | 行数 |
|------|------|------|
| `deepx-types/src/provider.rs` | 3 枚举(Thinking/Cache/UserSend) + EndpointSpec 加 7 字段 | ~35 |
| `deepx-config/src/registry.rs` | 注册 Qwen + GLM + Kimi | ~60 |
| `deepx-gate/src/types.rs` | ProviderConfig 传递适配字段 | ~20 |
| `deepx-gate/src/openai.rs` | chat_path/thinking/cache/balance 多分支 | ~45 |
| `deepx-msglp/src/lib.rs` | 构造 ProviderConfig 时查 EndpointSpec | ~5 |

**总计约 165 行 Rust，覆盖 DeepSeek + Qwen + GLM + Kimi。**

---

## MiMo (小米) 适配分析

**基础信息：**
- Base URL: `https://api.xiaomimimo.com/v1`
- Chat: `{base}/chat/completions`
- Models: `{base}/models`
- 文档: https://mimo.mi.com/docs/zh-CN/api/chat/openai-api

### 适配点差异矩阵（五家汇总）

| 适配点 | DeepSeek | GLM | Kimi | MiMo | Qwen |
|--------|----------|-----|------|------|------|
| **URL 前缀** | 无 | `/api/paas/v4` | `/v1` | `/v1` | `/compatible-mode/v1` |
| **Thinking** | `thinking: {type: "enabled"}` | identical ✓ | identical ✓ | identical ✓ | `enable_thinking: true` |
| **Cache 字段** | `prompt_cache_hit_tokens` | `prompt_tokens_details.cached_tokens` | `cached_tokens` | **无** (`prompt_tokens_details: {}`) | `prompt_tokens_details.cached_tokens` |
| **Balance** | `/user/balance` | 不支持 | `/users/me/balance` | 不支持 | 不支持 |
| **Reasoning** | `delta.reasoning_content` | ✓ | ✓ | ✓ | 待确认 |
| **Tool calls** | 标准 | ✓ | ✓ | ✓ | ✓ |
| **Stream 尾部 usage** | last delta chunk | last delta chunk | 独立 chunk + [DONE] | 独立 chunk + [DONE] | 待确认 |
| **额外限制** | 无 | 无 | 无 | ⚠️ thinking 模式下禁止传 temperature/top_p；多轮工具调用必须保留 reasoning_content | 无 |

### MiMo 核心差异

1. **无缓存信息** — `prompt_tokens_details: {}`（空对象），不支持 KV cache 统计。`CacheTokenField::None`。

2. **多轮工具调用限制** — 开启 thinking 且历史存在 tool_calls 时，后续请求的 assistant 消息必须包含 `reasoning_content`，否则返回 400。这是 MiMo 独有限制，影响 Agent 场景。

3. **默认 thinking = ON** — `mimo-v2.5-pro`/`mimo-v2.5`/`mimo-v2-pro`/`mimo-v2-omni` 默认开启思考。`mimo-v2-flash` 默认关闭。

4. **Model 版本变更** — `mimo-v2-flash` 即将下线（2026.6.30），自动转发至 V2.5。模型列表需更新：
   - 新增: `mimo-v2-omni`
   - 保留: `mimo-v2.5-pro`, `mimo-v2.5`, `mimo-v2-flash`
   - 移除: `mimo-v2-pro`（已升级到 V2.5）

5. **Stream 尾部 usage** — 流式响应的最后一个 chunk 有 `choices: []` 且带完整 `usage`（含 `completion_tokens_details.reasoning_tokens`）。当前 DeepX 的 usage 解析已在 SSE 循环中 `ev.get("usage")` 提取，天然兼容。

### 结论

**MiMo 是最接近 DeepSeek 的兼容层。** 当前 DeepX 配置已基本可用。与 DeepSeek 零差异适配点：
- Thinking 参数格式
- Reasoning delta 字段
- Tool calls 格式
- SSE stream 结构
- Auth (Bearer)

需要修正的只是：
- 缓存字段 → `None`
- model 列表更新（`mimo-v2-omni` 替换 `mimo-v2-pro`）
- 可选：Balance 绕过

---

## MiniMax (稀宇科技) 适配分析

**基础信息：**
- Base URL: `https://api.minimaxi.com/v1`
- Chat: `{base}/chat/completions`
- 文档: https://platform.minimaxi.com/docs/api-reference/text-openai-api

### 适配点差异矩阵（六家汇总）

| 适配点 | DeepSeek | Qwen | GLM | Kimi | MiMo | MiniMax |
|--------|----------|------|-----|------|------|---------|
| **URL 前缀** | 无 | `/compatible-mode/v1` | `/api/paas/v4` | `/v1` | `/v1` | `/v1` |
| **thinking type 值** | `"enabled"` | `enable_thinking: true` | `"enabled"` ✓ | `"enabled"` ✓ | `"enabled"` ✓ | `"adaptive"` ⚠️ |
| **reasoning 默认位置** | `delta.reasoning_content` | 待确认 | `delta.reasoning_content` | `delta.reasoning_content` | `delta.reasoning_content` | `<think>` 标签在 `content` 内 ⚠️ |
| **reasoning_split** | 不需要 | 不需要 | 不需要 | 不需要 | 不需要 | **需要 `reasoning_split: true`** |
| **Cache 字段** | `prompt_cache_hit_tokens` | `prompt_tokens_details.cached_tokens` | `prompt_tokens_details.cached_tokens` | `cached_tokens` | 无 | 待确认（文档未提及） |
| **M2.x thinking** | N/A | N/A | N/A | N/A | N/A | **不可关闭**（即使传 disabled） |
| **废弃参数** | `frequency/penalty` | 部分 | 部分 | 部分 | 部分 | `presence/freq_penalty`, `logit_bias`, `n` |

### MiniMax 核心差异

1. **thinking type 值不同** — MiniMax-M3 使用 `"adaptive"` 而非 `"enabled"`。这是唯一的取值差异。M2.x 模型 thinking 始终开启，即使传 `disabled` 也会忽略。

2. **reasoning 默认在 content 里** — 不加 `reasoning_split: true` 时，thinking 内容嵌入 `content` 字段中的 `<think>...</think>` 标签，不走 `reasoning_content` 字段。这意味着当前 DeepX 的 `reasoning_content` 解析会**完全收不到思考内容**。

3. **`reasoning_split: true` 必须传** — 要获得标准 `reasoning_content` 输出，必须额外传 `reasoning_split: true`。这个参数也在 `extra_body` 中。

4. **无 Cache 字段** — 文档未提及任何 KV cache 命中统计，`CacheTokenField::None`。

5. **`reasoning_details` 额外字段** — 开启 `reasoning_split` 后，thinking 会同时出现在 `reasoning_content` 和 `reasoning_details`（结构化数组）。只需用 `reasoning_content`。

### 请求 body 对比

**DeepSeek (当前):**
```json
{"thinking": {"type": "enabled"}, "reasoning_effort": "high", ...}
```

**MiniMax-M3:**
```json
{"thinking": {"type": "adaptive"}, "reasoning_split": true, ...}
```

### ThinkingParamMode 枚举最终版

```rust
pub enum ThinkingParamMode {
    OpenAi,             // thinking: {type: "enabled"} (DeepSeek, GLM, Kimi, MiMo)
    QwenEnableThinking, // enable_thinking: true (Qwen)
    MiniMaxAdaptive,    // thinking: {type: "adaptive"} + reasoning_split: true (MiniMax)
}
```

### 更新后的汇总

| 文件 | 改动 | 行数 |
|------|------|------|
| `deepx-types/src/provider.rs` | 4 枚举(Thinking/Cache/UserSend) + EndpointSpec 加 8 字段 | ~40 |
| `deepx-config/src/registry.rs` | 注册 Qwen + GLM + Kimi + MiniMax | ~75 |
| `deepx-gate/src/types.rs` | ProviderConfig 传递适配字段 | ~20 |
| `deepx-gate/src/openai.rs` | chat_path/thinking/cache/balance 多分支 | ~55 |
| `deepx-msglp/src/lib.rs` | 构造 ProviderConfig 时查 EndpointSpec | ~5 |

**总计约 195 行 Rust，覆盖 DeepSeek + Qwen + GLM + Kimi + MiMo + MiniMax。**

---

## Doubao/火山方舟 (字节跳动) 适配分析

**基础信息：**
- Base URL: `https://ark.cn-beijing.volces.com/api/v3`
- Chat: `{base}/chat/completions`
- 文档: https://www.volcengine.com/docs/82379/1399009?lang=zh

### 差异分析

| 适配点 | Doubao | DeepSeek | 兼容？ |
|--------|--------|----------|:---:|
| URL 前缀 | `/api/v3` | 无 | 需 `chat_path` |
| thinking 参数 | `thinking: {type: "enabled"/"disabled"}` | 同 | ✓ |
| reasoning_content | `delta.reasoning_content` | 同 | ✓ |
| tool_calls | 标准 OpenAI | 同 | ✓ |
| 默认 thinking | **disabled**（示例中显式关闭） | enabled | 注意 |
| 缓存字段 | 待确认 | `prompt_cache_hit_tokens` | ? |
| Balance | 待确认 | `/user/balance` | ? |
| Models | `/models` 待确认 | `/models` | ? |

**结论：仅 URL 前缀 `/api/v3` 需要适配。** thinking 格式与 DeepSeek 完全相同 (top-level `thinking: {type}`)。与 DeepSeek 的差异度最低——比 Kimi、MiMo 更接近基准。

### 更新后的汇总

| 文件 | 改动 | 行数 |
|------|------|------|
| `deepx-types/src/provider.rs` | 4 枚举 + EndpointSpec 加 8 字段 | ~40 |
| `deepx-config/src/registry.rs` | 注册 Qwen + GLM + Kimi + MiniMax + Doubao | ~85 |
| `deepx-gate/src/types.rs` | ProviderConfig 传递适配字段 | ~20 |
| `deepx-gate/src/openai.rs` | chat_path/thinking/cache/balance 多分支 | ~55 |
| `deepx-msglp/src/lib.rs` | 构造 ProviderConfig 时查 EndpointSpec | ~5 |

**总计约 205 行 Rust，覆盖 DeepSeek + Qwen + GLM + Kimi + MiMo + MiniMax + Doubao。**

---

## 附录 A: DeepSeek 官方 API 基准

**来源:** https://api-docs.deepseek.com/zh-cn/api/create-chat-completion

### 请求格式

| 参数 | 值 | 备注 |
|------|-----|------|
| Base URL | `https://api.deepseek.com` | 无 `/v1` |
| Chat 路径 | `/chat/completions` | 直接追加 |
| Auth | `Authorization: Bearer $KEY` | |
| `thinking` | `{"type": "enabled" \| "disabled"}` | top-level，默认 `enabled` |
| `reasoning_effort` | `"high"` \| `"max"` | top-level，默认 `high` |
| `user_id` | string ≤512, `[a-zA-Z0-9\-\_]` | 用于 KV cache 隔离 |
| `max_tokens` | integer | 输入+输出总长受上下文限制 |
| `stream` | boolean | SSE 流式 |
| `stream_options.include_usage` | boolean | 最后一个 chunk 前发 usage 统计 |
| `temperature` / `top_p` | number | 二选一 |
| **已废弃** | `frequency_penalty`, `presence_penalty` | 不再支持 |

### 响应 — 非流式 usage 字段

```json
{
  "usage": {
    "prompt_tokens": N,
    "completion_tokens": N,
    "total_tokens": N,
    "prompt_cache_hit_tokens": N,
    "prompt_cache_miss_tokens": N,
    "completion_tokens_details": {
      "reasoning_tokens": N
    }
  }
}
```

**KV cache 规则：** `prompt_tokens == prompt_cache_hit_tokens + prompt_cache_miss_tokens`

### 响应 — 流式 SSE

```
data: {"choices":[{"delta":{"content":"..."}}], ..., "usage": null}
...
data: {"choices":[{"delta":{},"finish_reason":"stop"}], ..., "usage": {"completion_tokens":9,"prompt_tokens":17,"total_tokens":26}}
data: [DONE]
```

特点：最后一个 delta chunk 的 `usage` 字段有完整统计，此前所有 chunk 的 `usage` 为 `null`。

### 模型列表

| 模型 | 状态 |
|------|------|
| `deepseek-v4-flash` | 推荐 |
| `deepseek-v4-pro` | 推荐 |
| `deepseek-chat` | 2026/07/24 弃用（→ v4-flash 非思考模式） |
| `deepseek-reasoner` | 2026/07/24 弃用（→ v4-flash 思考模式） |

### Balance

`GET https://api.deepseek.com/user/balance`

---

## 附录 B: 七家最终差异速查表

| 适配点 | DeepSeek | Qwen | GLM | Kimi | MiMo | MiniMax | Doubao |
|--------|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| URL 前缀 | 无 | `/compatible-mode/v1` | `/api/paas/v4` | `/v1` | `/v1` | `/v1` | `/api/v3` |
| thinking 参数 | `thinking: {type}` | `enable_thinking: bool` | 同基准 ✓ | 同基准 ✓ | 同基准 ✓ | `thinking: {type:"adaptive"}` | 同基准 ✓ |
| reasoning_effort | top-level | extra_body | N/A | 同基准 ✓ | 同基准 ✓ | N/A | 待确认 |
| reasoning 字段 | `delta.reasoning_content` | 待确认 | 同基准 ✓ | 同基准 ✓ | 同基准 ✓ | **需 `reasoning_split:true`** | 同基准 ✓ |
| cache.hit 字段 | `prompt_cache_hit_tokens` | `prompt_tokens_details.cached_tokens` | `prompt_tokens_details.cached_tokens` | `cached_tokens` | 无 | 无 | 待确认 |
| cache.miss 字段 | `prompt_cache_miss_tokens` | 无 | 无 | 无 | 无 | 无 | 待确认 |
| stream usage 位置 | last delta chunk | 待确认 | last delta chunk | 独立 chunk+[DONE] | 独立 chunk+[DONE] | 待确认 | 待确认 |
| balance | `/user/balance` | 无 | 无 | `/users/me/balance` | 无 | 无 | 待确认 |
| user_id 等价 | `user_id` body | 待确认 | 待确认 | `prompt_cache_key` | 无 | 无 | 待确认 |
| 特殊限制 | 无 | 无 | 无 | 无 | thinking 时禁 temperature/top_p | thinking 不可关(M2.x); `<think>` 标签 | 默认 thinking=disabled |
