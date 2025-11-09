 ### Project Specification: Flow Runtime Agent State & CLI Visualization


  1. 在执行期持久化记录“工作流与 Agent 阶段/状态”，支持恢复、复用与审计；
  2. 在命令行实时展示 Agent 列表的执行状态与进度（含进行中动画与完成标记）。

  — — —

  ### Part 1: The Essentials (Core Requirements for Any Project)

  #### 1.0 Project Overview (Required)

  - 1.1 Project Name: Flow Runtime State & Agent Visualization
  - 1.2 Project Goal: 为 codex-flow 的工作流执行增加可持久化状态文件与友好的 CLI 可视化，使执行过程可恢复、可追溯、可复用，用户能直观获知各 Agent 的执行状态与进度。
  - 1.3 Target Audience: 使用 codex-flow 的开发者与项目干系人；执行监控与故障定位的支持人员。

  #### 2.0 Core Functionality & User Journeys (Required)

  - 2.1 Core Features List
      - 运行时状态文件：系统 MUST 在 .codex-flow/runtime/state/ 维护工作流级状态 workflow_state.json，并为每个 Agent 维护 agents/{agent_id}.json。
      - 统一状态枚举：每个步骤/Agent MUST 使用 pending | in_progress | completed | failed | skipped，并记录时间戳与尝试次数。
      - CLI 可视化（TTY）：按工作流步骤顺序展示 Agent 列表：
          - 已完成：✅ <agent>
          - 进行中：<spinner> <agent>（循环动画，帧如 ⠋⠙⠸⠴⠦⠇ 或 -|/\）
          - 未执行：  <agent>（留空或中性标记）
      - 恢复执行：再次运行同一工作流时，系统 MUST 依据 workflow_state.json 跳过已完成步骤，从首个非完成步骤继续。
      - 状态快照：codex-flow status 输出最新快照（TTY 为列表视图；非 TTY 输出纯文本）。
      - 配置开关：提供 --status=tty|plain|off 与刷新频率（如 --fps=10 或 FLOW_STATUS_FPS=10）。
  - 2.2 User Journeys
      - 运行（TTY）：用户执行 codex-flow run -w <workflow.toml> → 系统创建/更新状态文件 → 终端实时展示列表（spinner 随进度刷新）→ 完成显示 ✅；失败显示 ❌ 并停止或进入策略化重试。
      - 运行（非 TTY）：降级为纯文本行式输出；状态文件照常更新。
      - 中断与恢复：再次执行同一工作流 → 跳过已完成步骤，从 in_progress 或下一 pending 继续。
      - 查询状态：codex-flow status → 读取 .codex-flow/runtime/state/ 并输出快照；若无状态，提示用户。

  #### 3.0 Data Models (Required)

  - WorkflowState
      - workflow_id (REQUIRED, string)
      - name (REQUIRED, string)
      - version (REQUIRED, string) — 工作流定义版本/指纹（可用哈希短码）
      - started_at (REQUIRED, RFC3339)
      - updated_at (REQUIRED, RFC3339)
      - status (REQUIRED, enum: running|completed|failed|canceled)
      - progress (REQUIRED, number 0..100)
      - steps (REQUIRED, array<StepState>)
      - artifacts (OPTIONAL, object) — 关键产物路径映射
      - attempt (REQUIRED, integer >=1)
  - StepState
      - id (REQUIRED, string) — 如 architecture、planning
      - title (REQUIRED, string)
      - status (REQUIRED, enum: pending|in_progress|completed|failed|skipped)
      - started_at (OPTIONAL, RFC3339)
      - ended_at (OPTIONAL, RFC3339)
      - attempts (REQUIRED, integer >=0)
      - last_error (OPTIONAL, string)
      - agent_state_file (REQUIRED, string) — 如 .codex-flow/runtime/state/agents/architecture.json
  - AgentState
      - agent_id (REQUIRED, string)
      - status (REQUIRED, enum 同上)
      - reasoning_effort (OPTIONAL, string)
      - stdout_tail (OPTIONAL, string) — 最近若干行输出（脱敏）
      - artifacts (OPTIONAL, array<string>)
      - metrics (OPTIONAL, object) — 耗时、重试次数等
      - updated_at (REQUIRED, RFC3339)

  #### 4.0 Essential Error Handling (Required)

  - 原子写入：状态写 MUST 采用“临时文件→rename”原子化；失败重试 3 次；仍失败 MUST 以非零码退出并提示。
  - 非 TTY：自动降级到纯文本；--status=tty 在非 TTY MUST 忽略并告警。
  - 并发写入：单执行过程 MUST 只有一处写工作流状态；未来并行步骤需文件锁或分片+合并策略。

  — — —

  ### Part 2: Advanced Specifications (For Complex or High-Fidelity Projects)

  #### 5.0 Formal Project Controls & Scope

  - 5.1 Document Control: Version 1.0 | Status: Draft | Date: 2025-11-08
  - 5.2 Detailed Scope
      - In Scope:
          - 创建/维护 .codex-flow/runtime/state/workflow_state.json 与 agents/*.json
          - CLI TTY 可视化（✅/spinner/空标识）与纯文本降级
          - codex-flow status 快照命令
          - 恢复执行（跳过已完成步骤）
      - Out of Scope:
          - 全屏 TUI 面板、运行历史时间线浏览器、Web 控制台
  - 5.3 Glossary
      - TTY：交互式终端
      - Spinner：字符帧动画

  #### 6.0 Granular & Traceable Requirements

  | ID | Requirement Name / User Story | Description | Priority |
  | :--- | :--- | :--- | :--- |
  | FR-001 | Workflow State File | MUST 维护 .codex-flow/runtime/state/workflow_state.json（元信息/步骤/进度）。 | Critical |
  | FR-002 | Agent State Files | MUST 为每个 Agent 维护 agents/{agent}.json。 | Critical |
  | FR-003 | TTY Visualization | MUST 在 TTY 列表展示：完成✅、进行中 spinner、未执行空标识。 | High |
  | FR-004 | Plain Fallback | 非 TTY SHOULD 纯文本输出，仍 MUST 更新状态文件。 | High |
  | FR-005 | Resume Execution | MUST 基于状态文件从首个非完成步骤续跑。 | Critical |
  | FR-006 | Status Command | codex-flow status 输出快照（TTY/纯文本）。 | High |
  | FR-007 | Atomic Writes | MUST 原子写，崩溃/中断后一致。 | Critical |
  | FR-008 | Config Knobs | --status=tty|plain|off、--fps=10/FLOW_STATUS_FPS。 | Medium |
  | FR-009 | Error Surfaces | 失败 MUST 标记 ❌ 与错误摘要；保留上下文供排障。 | High |

  #### 7.0 Measurable Non-Functional Requirements (NFRs)

  | ID | Category | Requirement | Metric / Acceptance Criteria |
  | :--- | :--- | :--- | :--- |
  | NFR-DUR-001 | Durability | 崩溃/断电后一致性 | 任意时刻状态文件 MUST 可解析；恢复 MUST 正确续跑。 |
  | NFR-PERF-001 | Performance | 刷新开销 | 10 Agents、5Hz 刷新下，CPU 额外开销 SHOULD < 5%；磁盘写 ≤ 5Hz。 |
  | NFR-UX-001 | UX | 动画体验 | Spinner 默认 8–12 FPS；闪烁最小化。 |
  | NFR-COMP-001 | Compatibility | 终端兼容 | macOS/Linux 常见终端 MUST 正常；Windows PowerShell SHOULD 降级可用。 |
  | NFR-OBS-001 | Observability | 可读性 | codex-flow status 在 80 列终端内 MUST 可读。 |

  #### 8.0 Technical & Architectural Constraints

  - 技术栈：Rust（codex-flow）；尽量使用原生 ANSI 控制/轻量 spinner，避免重型依赖。
  - 原则：状态与渲染解耦；状态为 source of truth；写入原子化；为并发步骤预留扩展。
  - 部署：跨平台（macOS/Linux/Windows）；状态路径固定在 .codex-flow/runtime/state/。

  #### 9.0 Assumptions, Dependencies & Risks

  - 假设：执行器具备生命周期钩子（step start/finish/fail）；文件系统支持原子 rename。
  - 依赖：TTY 检测；RFC3339 时间序列化。
  - 风险：并发步骤写冲突（需文件锁/队列）；部分终端不支持光标控制（需降级）。

  — — —

  ### Appendix A: File & CLI Examples

  1. 目录结构（示例）

  .codex-flow/
    runtime/
      state/
        workflow_state.json
        agents/
          architecture.json
          planning.json
          task_breakdown.json
          ...

  2. workflow_state.json 片段（示例）

  {
    "workflow_id": "agent_reasoning_upgrade",
    "name": "Agent Reasoning Upgrade",
    "version": "sha-abc1234",
    "status": "running",
    "started_at": "2025-11-08T10:00:00Z",
    "updated_at": "2025-11-08T10:00:05Z",
    "progress": 33.3,
    "attempt": 1,
    "steps": [
      {"id": "init", "title": "Init", "status": "completed", "attempts": 1, "agent_state_file": ".codex-flow/runtime/state/agents/init.json"},
      {"id": "architecture", "title": "Architecture", "status": "in_progress", "attempts": 1, "agent_state_file": ".codex-flow/runtime/state/agents/architecture.json"},
      {"id": "planning", "title": "Planning", "status": "pending", "attempts": 0, "agent_state_file": ".codex-flow/runtime/state/agents/planning.json"}
    ]
  }

  3. CLI 可视化（TTY）

  ✅ init
  ⠋ architecture
    planning
    task_breakdown
    context_manager
    code_generation
    validation
    runtime_prep

  注：spinner 帧可选 ⠋⠙⠸⠴⠦⠇ 或 -|/\。

  4. CLI 纯文本降级

  init            COMPLETED
  architecture    IN_PROGRESS
  planning        PENDING