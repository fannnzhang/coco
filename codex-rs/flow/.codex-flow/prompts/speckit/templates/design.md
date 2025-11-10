# 🧠 design.md — 方案设计规划模板

## 0. 元信息（Meta）

- Feature / Bug 名称：`<title>`
- Spec 路径：`spec/<feature-or-bug-name>/design.md`
- 版本 / 日期：`vX.Y · YYYY-MM-DD`
- 关联：`requirements.md`、`tasks.md`、项目文档
- 所有者 / 评审人：`<owner>` / `<reviewers>`
- 范围声明（Scope / Non-goals）
  - In Scope：`<本次包含>`
  - Out of Scope：`<明确不做>`

---


## 1. 方案概要（Solution Overview）

- 设计思路：`<概要>`
- 影响面：`<模块/页面/接口/数据/配置>`
- 兼容性/降级：`<说明>`
- 可观测性：`<日志/埋点/指标/告警>`

- 技术栈（Tech Stack）：
  - 架构模式：`<Clean / MVVM / Redux / …>`
  - 状态管理：`<Riverpod / Provider / Bloc / …>`
  - 网络层：`<Dio / Retrofit / HTTP / …>`
  - 本地存储：`<SQLite/Floor / Hive / MMKV / …>`
  - UI 框架/样式：`<Flutter / 组件库 / 主题系统>`

- 分层视图（示意）：
  ```
  Presentation (UI + State) → Domain (Use Cases) → Data (Repo + Sources + Models)
  ```

---

## 2. 模块与调用关系（Modules & Flows）（如需）

- 模块清单（新/改/复用）

- 核心调用链：`<文字/ASCII/时序图>`
- 状态机（如需）：`<状态与转移>`

- 模块按层划分（路径占位）
  - Presentation：`lib/pages/...` 或 `lib/features/.../presentation/...`
  - Domain：`lib/features/.../domain/...`
  - Data：`lib/features/.../data/...`

---

## 3. 数据与模型（Data & Models）（如需）

- 领域实体：`<字段/约束>`
- 存储与迁移（如需）：`<方案>`
- 隐私与敏感：`<处理>`

- 请求/响应模型（占位）
  - 请求：`RequestX { … }`（JSON 序列化约定）
  - 响应：`ResponseY { … }`（字段默认值与可空性）

---

## 4. 合同与集成（Contracts & Integrations）(如需)

- 失败与降级：`<策略>`
- 灰度与回滚：`<策略>`

- 接口示例（占位）
  ```
  @POST("/api/.../endpoint")
  Future<BaseResponse<T>> action(@Body() RequestX param);
  ```

---


## 5. 影响评估（Impact & Change List）（如需）

- 改动清单：`<模块/接口/脚本/配置>`
- 兼容性：`<说明>`
- 协作依赖：`<说明>`

---

## 6. 迁移与回滚（Migration & Rollback）（如需）

- 数据迁移：`<脚本/回滚>`
- 配置/版本控制：`<策略>`
- 切换计划：`<方案>`

- 数据库迁移占位：
  ```
  Migration(X, Y, (db) async {
    await db.execute('ALTER TABLE <table> ADD COLUMN <col> <type> DEFAULT <v>');
  });
  ```

---



## 7. 代码组织与约定（Code Map & Conventions）

- 目录与命名：`<对齐项目约定>`
- 注释与文档：`<对齐项目约定>`

---

## 8. 评审清单（Review Checklist）

- [ ] 与 `requirements.md` 对齐
- [ ] 每个 Phase 可运行/可测试/可回滚
- [ ] 兼容性与风险明确

---