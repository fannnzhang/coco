# Codex Flow 技术设计（初版）

本设计文档定义了在 `codex-rs` 工作区新增 `codex-flow` crate 的整体方案。`codex-flow` 提供一个简版的多 Agent 协同编排工具，统一对接不同底层引擎（当前目标：`codex` 与 `codemachine`），以 TOML 描述 Agent 与 Workflow，支持 `init` 初始化脚手架与 `run` 运行工作流。为节省测试成本，首期默认提供 Mock 执行模式（避免真实模型调用）。

## 目标与范围

- 以最小可用集（MVP）实现：
  - 解析一个最小 Workflow（含一到多个 step），并可运行。
  - 支持两类 Engine：`codex`、`codemachine`，首期以 Mock 适配器为主，真实执行适配器接口预留。
  - 使用 TOML 进行配置（`agents`、`workflows`）。
  - 提供 `init`：在工程中生成默认的 `.codex-flow/` 目录、默认 Agent/Workflow 配置与可编辑的 prompt 模板。
  - `run`：在 Mock 模式下执行 Workflow，写入/打印产出，便于本地验证。
- 非目标（后续版本再迭代）：
  - 并发/分支/回滚等高级编排能力。
  - 多模型路由、工具调用(Function Calling)的完整实现。
  - 跨进程长会话状态管理与缓存。

## 关键术语

- Engine：底层能力的统一抽象（如 `codex` CLI、`CodeMachine-CLI` 等）。
- Agent：基于某个 Engine 的具体调用单元，常见属性包括模型名、prompt 模板路径、默认参数等。
- Workflow：由若干顺序 Step 组成的可执行流程。
- Step：引用一个 Agent，并声明该步的输入、输出与变量绑定。

## 架构概览

- CLI：`codex-flow` 二进制，提供子命令 `init`、`run`。
- Flow Runner：读取配置，构建执行图（首期为线性步骤），逐步调度。
- Engine Registry：注册并选择 `codex` / `codemachine` / `mock` 引擎实现。
- Engine Adapter：每类引擎一个适配器（进程调用或纯内存 Mock）。
- Process Invoker：统一的子进程调用与 IO 归一（真实模式时使用）。

首期默认走 MockEngine，真实 Engine 走通参数拼装与 IO 结构，但可通过显式标志关闭或留空以避免消耗真实配额。

## 与 CodeMachine 工作流的对应关系

- 参考：`CodeMachine-CLI/templates/workflows/_example.workflow.js`。
- 映射关系：
  - JS 模板中的 agent 与 step 用代码描述；本方案用 TOML 的 `[agents.*]` 和 `[[workflow.steps]]` 描述。
  - JS 中按序执行的 steps 对应 TOML 中的 `workflow.steps` 顺序数组。
  - Prompt 模板路径默认指向 `.codex-flow/prompts/...`，`init` 会把编译进二进制的模板落盘到该目录。
  - 后续可扩展 step 的输入/输出/控制字段以覆盖更多 JS 模板能力。

## 配置设计（TOML）

工作流文件建议存放在 `.codex-flow/workflows/*.workflow.toml`，也可任意路径传入 `run`。

建议使用表与数组表描述 Agents 与 Workflow（单文件单工作流）：

```toml
# 顶层元信息（可选）
name = "commit_flow"

[defaults]
engine = "codex"          # 默认引擎，可被每个 agent 覆盖
mock = true                # 缺省用 mock 模式运行，节省成本

[engines.codex]
# 可选：真实执行时的命令模板与默认参数
# bin = "codex"           # CLI 可执行文件名或绝对路径
# args = ["chat", "--stdio"]

[engines.codemachine]
# bin = "codemachine"     # 例如 CodeMachine-CLI 的入口
# args = ["run", "--stdio"]

[agents.commit]
engine = "codex"          # 可为 "codex" 或 "codemachine"，也可省略用 defaults.engine
model = "gpt-5"
prompt = "prompts/templates/codemachine/agents/01-architecture-agent.md"

[workflow]
description = "从 git diff 生成提交信息"

  [[workflow.steps]]
  agent = "commit"                 # 引用上面的 agents.commit
  description = "commit-message"   # 可选：用于 CLI 展示与日志命名
  # 可选：在 step 上覆盖 engine/model/prompt（mock/真实皆支持覆盖）
  # model = "gpt-4.1"
  # prompt = "prompts/templates/.../custom.md"
  # engine = "codemachine"
  # Mock 模式下不读取 input/output，仅打印 Shell 命令
  # [workflow.steps.input]
  # template = "..."
  # [workflow.steps.output]
  # kind = "stdout" | "file"
  # path = "..."

# 可选：顶层变量（运行时也可通过 CLI 传入覆盖）
[vars]
# diff = "..."
```

说明：
- `agents.<id>`：定义单个 Agent。至少包含 `prompt`，建议包含 `engine`、`model`。
- `workflow.steps`：数组表，按序执行。每个 step 的 `agent` 对应一个 agent id。
- `workflow.steps[*].description`：可选的人类可读描述，用于 CLI 输出与 `.codex-flow/runtime/debug/` JSON 日志文件命名；留空时退回到 `agent` 名称。
- 覆盖：在 step 上书写 `engine`/`model`/`prompt` 字段可覆盖引用的 agent 默认值。
- Mock 执行：仅打印 Shell 命令（不读取 `input`/`output`）。
- 真实执行（规划中）：`input` 参与渲染，`output` 决定落盘位置。

兼容：也支持早期多工作流格式（`[workflows.<name>]`），`run <file>` 解析失败时将回退并选择第一个工作流执行。

## Prompt 与模板来源

- 初始化时将把默认模板复制到目标工程 `.codex-flow/prompts/` 下。
- 首期直接复用仓库 `.codex-flow/prompts/` 下的 Markdown 模板（编译进二进制），按原目录结构拷贝：
  - `codemachine/agents/*`
  - `codemachine/workflows/*`
  - `codemachine/output-formats/*`
  - 以及需要的 `dev-codemachine/*` 或 `test-workflows/*`
- 复制完成后，`flow.toml` 内的 `prompt` 路径将指向 `.codex-flow/prompts/...`，方便接入方就地微调。

## CLI 设计

- `codex-flow init [--dir <path>]`
  - 若未指定 `--dir`，默认在当前项目根下创建 `.codex-flow/`。
- 生成：`.codex-flow/flow.toml`、`.codex-flow/prompts/`（复制自内置模板，除非通过 `--templates-dir` 指定其它目录）。
  - 不覆盖已存在文件，除非 `--force`。

- `codex-flow run <workflow.toml> [--mock | --no-mock] [--verbose]`
  - 解析单个工作流文件（TOML），逐步执行 `workflow.steps`。
  - `--mock` 为无参开关，显式开启 Mock；`--no-mock` 关闭 Mock（二者互斥）。若均未指定则回退到配置文件的 `defaults.mock` 或内建默认值。
  - Mock（默认开启，除非 `--no-mock` 或配置中明确关闭）：不调用真实模型，而是读取 `.codex-flow/runtime/debug/{index}-{step}.json` 中的历史事件，并以每秒一行 JSON 的节奏回放，将事件交给 human renderer 解析展示，进而模拟真实 `codex exec --json` 的流式输出。
  - 真实模式：按引擎适配器真正执行。对 `codex` 引擎会自动启用 `codex exec --json`，无需额外 CLI 选项。

## 执行与输出管控（真实模式雏形）

为支撑 `codex` 等真实引擎的落地，本期在设计上补充如下约束：

- **统一的引擎执行器**：在 runner 中新增 Engine Runner 抽象，负责构建并启动子进程，读取配置中的 `engines.<engine>` 信息决定可执行文件与默认参数，逐步扩展每类引擎的执行协议。
- **JSON 事件管线**：`codex` 引擎统一走 `codex exec --json`（追加配置提供的自定义参数），将 stdout 解析为 JSONL 事件并回传给上层：
  - runner 在内部捕获 stdout/stderr，避免原始内容直通终端；
  - 解析到的事件会实时推送给渲染层，以便即时展示与后续做快照/日志；
  - stderr 内容（如模型网络错误）同样被截获并统一输出，便于调用方处理。
- **人类可读展示**：基于 JSON 事件做流式渲染，复用 `codex exec` 的输出风格（命令启动、增量输出、最终消息），保证用户在 shell 中能实时看到执行进度，而不是在任务结束后一次性刷屏。
- **步骤结果记录**：runner 会跟踪每个 step 的退出状态；若进程失败将立即中止，并保留事件日志便于事后排查。
- **执行日志落盘**：
  - 流式事件：每个真实步骤将 codex `--json` 事件实时写入 `.codex-flow/runtime/debug/{index}-{agent}-agent.json`，便于回放与调试；同时，人类可见的 shell 输出会被追加到 `.codex-flow/runtime/logs/{index}-{agent}-agent.log`。
  - 最终结果（Memory）：利用 `codex exec` 原生的 `-o <file>` 选项，将该 Agent 的“最后一条消息”落盘为 Markdown，总结文件写入 `.codex-flow/runtime/memory/{index}-{agent}-agent-result.md`，供后续作为更有价值、言简意赅的 Memory 资料复用到上下文中。Mock 模式下会在回放完成后，根据最后一个 `agent_message` 事件同样生成该 Markdown 文件。

  - 说明：`.codex-flow/runtime/memory/` 目录后续将用于存放更有价值、言简意赅的 Markdown 记忆文档（供上下文复用）；为避免 JSON 事件污染上下文，JSON 回放日志统一迁移到 `.codex-flow/runtime/debug/`。

## 引擎抽象与适配

统一接口（概念性）：

```rust
trait Engine {
    fn name(&self) -> &str;
    fn invoke(&self, agent: &AgentSpec, input: &str, vars: &Vars) -> Result<String>;
}
```

- MockEngine：
  - 不进行任何网络调用。
- 每个 step 会按 `{index}-{slug}.json` 的命名约定在 `.codex-flow/runtime/debug/` 下寻找日志文件，`slug` 来源于 `description`（若为空则回退到 agent 名）。
- 每个 step 同时会生成 `.codex-flow/runtime/memory/{index}-{slug}-agent-result.md` 的 Markdown 结果文件。
  - 找到日志后逐行读取 JSON 事件（忽略非 JSON 行），并在事件之间按 1 秒的节奏发送给 HumanEventRenderer，从而复刻真实 `codex exec --json` 的流式体验。
  - 如果对应日志不存在或为空，则直接报错，提示先以真实模式运行一次来生成 memory。
  - 回放得到的 JSON 不直接原样输出，而是交由统一的 HumanEventRenderer（`human_output` 层）消费，这样上层展示逻辑无需感知底层使用真实 engine 还是 mock replay。
- CodexEngine（占位）：
  - 真实模式下通过子进程调用 `codex` CLI（命令、参数可在 `[engines.codex]` 中配置）。
  - 首期仅约定最简单的“单轮 prompt → 文本输出”通道；复杂功能后续扩展。
- CodeMachineEngine（占位）：
  - 真实模式下通过子进程调用 `codemachine` CLI。与上同，先打通最小输入/输出。

> 说明：首期默认运行在 MockEngine，真实 Engine 的命令行协议作为可配置项预留，不在本期消耗真实模型资源。

## 输入输出与变量

- 变量合并优先级：CLI `--var` > `vars-file` > 配置文件 `[vars]`。
- Step 输入：当前以 `template` 渲染字符串作为输入；未来支持 `file`（`@path`）、`stdin`（`@-`）等形式。
- Step 输出：
  - `stdout`：直接打印；
  - `file`：写入目标路径（默认覆盖，后续支持 `append = true`）。

## 初始化策略（Scaffold）

- 复制模板：将编译进二进制的模板写入 `.codex-flow/prompts/`（或按 `--templates-dir` 自定义来源）。
- 生成基础配置：

```toml
# .codex-flow/flow.toml（示例）
[defaults]
engine = "codex"
mock = true

[agents.commit]
engine = "codex"
model = "gpt-5"
prompt = "prompts/templates/codemachine/agents/01-architecture-agent.md"

[workflows.commit_flow]
description = "从 git diff 生成提交信息"

  [[workflows.commit_flow.steps]]
  use = "commit"
  [workflows.commit_flow.steps.input]
  template = "请基于以下 diff 生成规范化提交信息:\n\n{{diff}}"
  [workflows.commit_flow.steps.output]
  kind = "file"
  path = "COMMIT_MESSAGE.md"
```

> 接入方在 `.codex-flow/` 中直接修改模板与配置即可，无需改动编排引擎代码，降低理解成本。

## 日志与可观测性

- `--verbose` 输出每步的：选用引擎、agent 配置摘要、输入长度、输出摘要与落盘路径。
- Mock 模式下打印 Mock 命中来源（mocks 文件/模板渲染/占位文本）。

## 错误处理

- 配置校验：
  - 缺失必须字段（如 `use`、`prompt`）时，报错并标注路径（`workflows.<name>.steps[i]`）。
  - 路径不存在（`prompt`、`mocks`），在 Mock 模式下给出警告并回退到占位响应；真实模式下报错。
- 执行失败：
  - 子进程执行（真实模式）失败时回传标准错误与退出码。
  - 允许 `--continue-on-error`（后续）以跳过失败步骤。

## 测试与验证

- 配置解析单测：对合法/非法 TOML 做覆盖。
- Runner 单测：在 MockEngine 下运行示例 Workflow，断言输出文件内容与日志。
- 快照测试：使用 `insta` 对 Mock 输出做快照（无需真实模型）。
- 集成/端到端：`codex-flow run .codex-flow/workflows/commit.workflow.toml --mock`，断言输出的 Shell 命令符合预期。

## 代码结构（拟定）

```
codex-rs/codex-flow/
  ├── Cargo.toml
  ├── src/
  │   ├── main.rs             # clap CLI: init/run
  │   ├── config.rs           # TOML 定义（serde）与加载
  │   ├── runner.rs           # 线性 workflow 执行器
  │   ├── engines/
  │   │   ├── mod.rs
  │   │   ├── mock.rs         # MockEngine
  │   │   ├── codex.rs        # CodexEngine（占位）
  │   │   └── codemachine.rs  # CodeMachineEngine（占位）
  │   ├── init.rs             # 脚手架复制与默认 flow.toml 生成
  │   └── util/
  │       └── process.rs      # 子进程调用与 IO 归一
  └── tests/
      └── smoke.rs
```

- 依赖（工作区复用）：
  - `anyhow`、`serde`、`serde_json`、`toml`
  - `clap`、`clap_complete`
  - `walkdir`、`fs_extra`（或 `ignore` + `copy_dir` 实现）
  - `handlebars` 或 `tinytemplate`（变量渲染）
  - `insta`（测试）

> 遵循工作区规范：crate 命名为 `codex-flow`；尽量使用工作区依赖（`{ workspace = true }`）。

## 兼容与扩展

- CLI 适配层参数化：`[engines.<name>]` 可配置可执行名与默认参数，未来支持 `env` 注入与超时设置。
- Step 能力扩展：并发、条件、循环、工具调用（shell/http）等作为后续 feature。
- 输出路由：支持多输出（stdout+file）、命名管道/JSON 结构化输出（后续）。

## 里程碑与实施计划

1) MVP（本次）：
- 定义配置结构体与解析（单文件单工作流 + 兼容多工作流）；
- `init` 复制模板并生成示例 `.codex-flow/workflows/commit.workflow.toml`；
- `run --mock` 线性执行并打印 Shell 命令（不调用模型）；
- 单测 + 快照测试。

2) 实引擎打通（可选）：
- `codex`/`codemachine` 子进程调用打通最小链路（受限于本地可用 CLI）。

3) 体验与健壮性：
- 错误信息、`--verbose`、路径相对性与覆盖策略；
- 更多输入源（文件/stdin）与输出模式（追加）。

## 示例：最小运行

- 初始化：

```
# 在目标工程根目录
codex-flow init
```

- 运行工作流（Mock，打印 Shell 命令，不调用模型）：

```
codex-flow run .codex-flow/workflows/commit.workflow.toml --mock
# 输出示例：
#   cat "prompts/templates/codemachine/agents/01-architecture-agent.md" | codex exec --model gpt-5
```

以上即为 `codex-flow` 初版的技术设计，聚焦“配置 → 解析 → Mock 执行 → 可落盘”的闭环，默认将 CodeMachine 的 prompt 模板复制到工程，使接入方在 `.codex-flow/` 目录内按需微调即可。
