#### 1.0 需求概述
- 1.1 需求名称：为当前 flow 工具新增“workflow 中的 agent 状态记录 + 流程中断后继续执行”，不浪费之前 agent 获取到的信息，且不会因为重新执行消耗更多 token。（原文用语保留）
- 1.2 背景痛点：
  - 之前 agent 获取到的信息在流程中断后容易丢失或无法复用（“不浪费之前 agent 获取到的信息”）。
  - 重新执行会产生额外 token 消耗（“不会因为重新执行消耗更多 token”）。
- 1.3 目标与收益：
  - 流程中断后 MUST 继续执行。（FR-002）
  - 历史 agent 信息 MUST 保留可复用。（FR-001）
  - 重新执行 MUST NOT 产生额外 token 消耗。（FR-003）

#### 2.0 功能范围
- 2.1 Agent 状态记录（FR-001）
  - 记录时机：未提供。
  - 记录内容：原文仅称“agent 获取到的信息”；具体字段未提供。
  - 说明：术语“Agent/Workflow/Step”参见项目背景文档（Source: codex-flow-design.md）。
- 2.2 中断恢复流程（FR-002, FR-004）
  - 流程中断触发条件：未提供（示例：进程退出、人工中止等，待确认）。
  - 恢复入口与步骤：恢复点 SHOULD 紧接最后成功的 agent；已完成步骤 MUST NOT 重复执行。（由“继续执行”“不浪费之前信息”推导）
- 2.3 Token 成本控制（FR-003, NFR-001）
  - 限制：恢复执行 MUST NOT 带来额外 token 消耗；不得因重复调用已完成 agent 而产生冗余费用。

- 2.4 CLI 恢复入口（FR-004）
  - `codex-flow resume <workflow>.toml --run-id <RUN_ID>` 为默认入口，mock 与 real 引擎均可直接读取 `.codex-flow/runtime/state/<workflow>/<run-id>.resume.json`。
  - `codex-flow run ... --resume-from <state.json>` 支持在新进程中沿用既有 `resume_pointer` 并跳过已完成步骤。
  - `codex-flow state prune --days <N>` 提供运维级清理能力并输出磁盘占用统计；README 模板会随命令刷新，保持手册一致。
  - 紧急回滚可通过 `CODEX_RESUME_DISABLED=1 codex-flow ...` 临时禁用状态写入，但默认必须保持开启以满足 FR-001~FR-004。

#### 3.0 用户旅程
> 采用“用户动作 → Flow 工具响应 → 结果”，严格复述原始需求语义并结合背景术语。

1. 用户触发 workflow → 系统记录当前 agent 状态（可复用的“agent 获取到的信息”）→ 结果：状态可供后续恢复。（FR-001；术语来源：codex-flow-design.md）
2. 用户因中断重新启动 → 系统定位最后成功的 agent 并从其后继续执行 → 结果：不重复已完成步骤，避免额外 token 消耗。（FR-002, FR-003）

#### 4.0 状态数据与存储
- 4.1 状态内容：原文仅有“agent 获取到的信息”；除该表述外，具体字段（如 step 索引/完成清单/上下文片段等）未提供。
- 4.2 存储介质 / 目录：未提供。
- 4.3 一致性要求：不得浪费之前的信息（原文语义）。

#### 5.0 异常与恢复策略
- 5.1 中断场景：
  - 文档版本：0.1.0；状态：Draft；日期：2025-11-11。
  - 中断类型：未提供。
- 5.2 恢复约束：
  - MUST 从中断点（最后成功 agent 之后）继续执行。（FR-002）
  - MUST NOT 重复执行已完成步骤；由此避免额外 token 消耗。（FR-003）

#### 6.0 验收标准
- 成功恢复一次中断 workflow 时 MUST 无信息丢失（“agent 获取到的信息”可复用）。（FR-001, FR-002）
- 单次恢复 MUST NOT 产生额外 token 消耗。（FR-003）
- 其他标准：未提供。

#### 7.0 范围与风险
- 7.1 In Scope：
  - Agent 状态记录（FR-001）。
  - 中断后继续执行（从最后成功的 agent 之后继续）（FR-002）。
  - 避免重复执行以控制 token 成本（FR-003）。
- 7.2 Out of Scope：
  - 用户未指定；项目背景文档将并发/分支/回滚等作为后续能力（仅作范围理解，不引入本次范围）。（Source: codex-flow-design.md）
- 7.3 未决 / 依赖（Open Questions）
  - Q1 具体需要记录的“agent 获取到的信息”字段列表？（未提供）
  - Q2 状态存储的介质/目录/生命周期与清理策略？（未提供）
  - Q3 中断类型与检测/上报机制？自动恢复 vs. 显式恢复入口？（未提供）
  - Q4 允许的最小重放范围与 token 计量口径（如何定义“额外消耗”）？（未提供）
  - Q5 与 codex-flow 引擎/适配层的接口对齐（仅术语澄清，用于实现阶段分工，不扩展范围）。（Source: codex-flow-design.md, README.md）

附录 A：术语对齐（解释性，非需求）
- Agent：基于某个 Engine 的具体调用单元（Source: codex-flow-design.md）。
- Workflow/Step：顺序执行的步骤与流程（Source: codex-flow-design.md）。
- Engine：底层能力抽象，如 codex/codemachine/mock（Source: codex-flow-design.md）。

附录 B：需求编号（稳定 ID）
- 功能性（FR）
  - FR-001 记录并保留可复用的历史 agent 信息（“agent 获取到的信息”）。
  - FR-002 中断后从最后成功的 agent 之后继续执行。
  - FR-003 恢复执行不得重复已完成步骤，不得产生额外 token 消耗。
  - FR-004 定义恢复入口与步骤（占位；待用户澄清）。
- 非功能性（NFR）
  - NFR-001 Token 成本约束：恢复不得带来额外 token 消耗（与 FR-003 对应，用于量化约束）。
