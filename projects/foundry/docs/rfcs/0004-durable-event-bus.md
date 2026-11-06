# RFC 0004：持久事件总线

- 状态：Draft，设计待评审
- 日期：2026-11-04
- 关联：[ADR 0001 Rust workspace 与六边形架构](../adr/0001-rust-workspace-and-hexagonal-architecture.md)、[ADR 0003 Git 存内容，Postgres 存状态](../adr/0003-git-content-postgres-state.md)、[RFC 0003 Task Workflow 模型](0003-task-workflow-model.md)

## 0. 结论

Foundry 需要一个平台级的持久事件总线，而不是让每个模块各自实现通知、轮询或投递。事件只写一份到现有 `events` 日志；每个 consumer 用 `(consumer_id, partition_key)` 游标记录自己处理到哪里；dispatcher 按订阅和分区拉取未处理事件，在分区锁内交给 consumer。

Task workflow 只是 `task-workflow` consumer，不是总线所有者；通知、投影、外部 webhook、PM 等模块复用同一套设施。

核心决定：

1. `events` 是唯一的事件日志和投递源，不复制事件。
2. 订阅在代码中注册，不建 subscription 表。
3. 每个 consumer/partition 一个持久游标。
4. 事务型 consumer 的效果、事件追加、游标推进在同一个事务中，等价 exactly-once。
5. 外部 consumer 是 at-least-once，必须自己幂等。
6. 同一 partition 按 `seq` 顺序处理；不同 partition 可以并行。
7. 锁顺序固定为游标 → 聚合行；命令路径也必须走 `process_partition`。
8. 不做 per-consumer 事件副本，不做 outbox 式 `attempts` / `available_at` / `published_at`。

## 1. 目标与非目标

### 1.1 目标

- 为所有需要“事件发生后做点什么”的模块提供统一、持久、可重试的设施。
- 事件只存一份；新增 consumer 不需要改动生产者。
- 支持本地事务型 consumer（效果 exactly-once）和外部 consumer（at-least-once）。
- 支持按聚合/项目/全局分区并行，同时保证分区内顺序。
- 多实例安全；consumer 失败只影响自己的分区。
- 与现有 Postgres、六边形架构、`events` 审计日志一致。

### 1.2 非目标

- 不做通用消息中间件，不替代 NATS/Kafka。
- 首期不实现 dead-letter、暂停/跳过、消费者动态注册 UI。
- 首期不实现跨数据中心复制、事件压缩、schema registry。
- 不向外部客户端直接暴露 publish；外部只能通过用例/命令产生事件。
- 不重新引入 outbox 的 per-event 投递表。

## 2. 术语与不变量

| 术语 | 含义 |
|---|---|
| event log | 现有 `events` 表；追加式、不可变、按 `seq` 全序。 |
| trigger event | 会触发某个 consumer 处理的事件类型；由订阅的 `kinds` 过滤。 |
| audit event | 只用于历史/审计、不触发 workflow 的事件。 |
| subscription | 代码中注册的消费者声明：`consumer_id`、事件类型、分区键、投递模式。 |
| partition | 订阅内的并行单元；如 `task_id`、`project_id` 或 global。 |
| consumer cursor | `(consumer_id, partition_key) -> last_seq`，记录已处理的触发事件。 |
| dispatcher | 平台后台组件，按订阅/分区拉取并投递事件。 |
| transactional consumer | 事件处理与游标在同一数据库事务内完成的消费者。 |
| at-least-once consumer | 副作用成功后才推进游标的外部消费者；可能重复。 |

不变量：

1. 事件只写一份，consumer 不复制事件。
2. 事件不可修改；游标是 consumer 的读位置，不是事件状态。
3. 同一 `(consumer_id, partition_key)` 同时只有一个处理者。
4. 同一 partition 内按 `seq` 顺序处理。
5. 事务型 consumer 的游标推进与效果在同一事务内。
6. 游标只推进到本次实际读取的触发事件，不能简单取 `max(seq)`。
7. consumer 不得跨 partition 修改另一个 consumer 的游标。
8. 事件总线的 append 不自己开事务；必须加入调用方的事务。

## 3. 架构

### 3.1 事件日志

复用现有 `events` 表：

```sql
seq          bigserial
id           uuid primary key
project_id   uuid null
aggregate_id uuid not null
kind         text not null
actor        jsonb not null
payload      jsonb not null
occurred_at  timestamptz not null
```

- 只追加，不更新、不删除；修正通过新事件表达。
- `seq` 由 Postgres 分配，是 consumer 使用的全序。
- `aggregate_id` 是聚合分区的键；`project_id` 是项目分区的键；项目级事件可能为 null。
- payload 对总线不透明；consumer 按 `kind` 解释，schema由领域模块负责。
- 事件总线是内部设施；外部客户端不能直接 publish，只能通过用例产生事件。

### 3.2 订阅

```text
Subscription
  consumer_id      稳定标识，如 "task-workflow"
  kinds            订阅的触发事件类型
  partition_by     Aggregate | Project | Global
  delivery         Transactional | AtLeastOnce
  initial_position Beginning | Latest
```

规则：

1. 订阅在启动时由组合根注册，不建 subscription 表。
2. `consumer_id` 稳定；改变 `kinds`、`partition_by` 或投递语义时，使用新的 id 或显式迁移游标。
3. 一个 consumer 首期只有一个订阅；需要多个处理逻辑时注册多个 `consumer_id`。
4. `kinds` 只列触发事件；审计事件不应出现在订阅中，除非它同时也是该 consumer 的触发事件。
5. `partition_by = Aggregate` 使用 `aggregate_id`；`Project` 使用 `project_id`（为 null 的事件跳过）；`Global` 使用固定键 `global`。
6. 首期只支持 `initial_position = Beginning`（游标从 0 开始）。新增 consumer 的“从注册时开始”需要持久化起始位置，留到有实际需求时再做。

### 3.3 消费游标

```sql
create table event_consumer_cursors (
    consumer_id   text not null,
    partition_key text not null,
    last_seq      bigint not null default 0,
    updated_at    timestamptz not null default now(),
    primary key (consumer_id, partition_key)
);
```

- `partition_key` 是聚合/项目 id 的字符串形式，或 `global`。
- 游标行首次处理时懒创建；缺失视为 `last_seq = 0`。
- 游标不是事件，不进入 `events`，也不进入 `Task.operations`。
- 游标只由 dispatcher 或 `process_partition` 在 consumer 的事务内更新。
- 事务型 consumer 的游标更新与效果同事务；at-least-once consumer 在副作用成功后单独推进。
- 已删除 consumer 的游标行首期手工清理，不做自动 GC。

### 3.4 分发器

dispatcher 是 `services/control-plane` 中的后台循环，也可以作为独立进程；多实例安全。

循环：

```text
for each subscription:
  partitions = find partitions with trigger events seq > cursor
  for each partition:
    process_partition(consumer_id, partition_key, command = None)
sleep(poll_interval)
```

`process_partition`：

```text
begin transaction
  lock/insert cursor (consumer_id, partition_key) for update
  command?.(uow)                       // 可选：追加输入事件/修改聚合
  events = select trigger events
             where partition = key and seq > last_seq
             order by seq limit batch
  for e in events:
    consumer.handle(uow, e)
  cursor.last_seq = last processed event seq
commit
```

规则：

1. 多实例用 `SELECT ... FOR UPDATE SKIP LOCKED` 锁游标行；抢不到就跳过，下轮再试。
2. dispatcher 不在事务内做外部 I/O；外部 consumer 使用 at-least-once，先提交读事务，再做副作用，最后单独推进游标。
3. 候选分区查询由 Postgres adapter 实现；需要 `events(kind, seq)` 索引，具体查询是实现细节。
4. 轮询间隔、批大小可配置。首期用轮询；`LISTEN/NOTIFY` 可作为延迟优化，但不能作为唯一机制。
5. dispatcher 不递归处理另一个 partition；跨 partition 的效果写成新事件，由对应 partition 的 dispatcher 处理。

## 4. 公共契约

事件总线对外只暴露以下能力：

- `EventLog::append(uow, events)`：在调用方的事务内追加事件，不自己开事务。
- `EventDispatcher::register(consumer)`：启动时注册 consumer。
- `EventDispatcher::process_partition(consumer_id, partition_key, command)`：dispatcher 和同步命令路径的共同入口。
- `EventConsumer::handle(uow, event)`：consumer 的处理逻辑。
- `UnitOfWork`：事务上下文，至少包含事件追加；领域仓库通过同一事务上下文访问。

约束：

1. 命令路径不得自己开事务，也不得在调用 `process_partition` 之前锁聚合行。
2. `process_partition` 先锁游标，再执行 `command`；`command` 内可以锁聚合行、追加事件、修改状态。
3. dispatcher 在同一事务内推进游标；任何 handler 失败都回滚整个事务。
4. 事务型 consumer 不得在事务内调用外部系统。
5. 外部 consumer 不持有数据库事务做 I/O；重复投递由消费者幂等处理。

示意接口（实现可以调整，契约不变）：

```rust
pub trait EventConsumer: Send + Sync {
    fn subscription(&self) -> Subscription;

    async fn handle(
        &self,
        uow: &mut dyn UnitOfWork,
        event: &StoredEvent,
    ) -> Result<(), ConsumerError>;
}

pub trait EventDispatcher: Send + Sync {
    async fn register(&self, consumer: Arc<dyn EventConsumer>);

    async fn process_partition(
        &self,
        consumer_id: &ConsumerId,
        partition_key: &PartitionKey,
        command: Option<&mut dyn PartitionCommand>,
    ) -> Result<(), DispatchError>;
}
```

## 5. 事件分类

总线本身不强制全局的 trigger/audit 标志；每个订阅的 `kinds` 决定它消费哪些事件。领域模块负责声明哪些事件是触发事件。

Task workflow 的首期分类：

| 类别 | 事件 |
|---|---|
| 触发 | `TaskCreated`、`InputReceived`、`ActorBound`、`ChildTerminal`、`DispatchRequested` |
| 仅审计 | `TaskStateChanged`、`TaskStageChanged`、`TaskCompleted`、`ActiveActorsChanged`、`ChildCreated` 等效果事件 |

- `ChildTerminal`、`DispatchRequested` 既是触发事件，也出现在 `Task.operations`。
- `ConfigChanged` 首期不是 Task workflow 的触发事件；它只影响后续验收点，不回溯已进入的验收。若未来需要唤醒 workflow，再把它加入订阅并定义 fan-out。
- 生产者不知道有哪些 consumer；它只追加事件，总线按订阅路由。

## 6. 投递语义

### 6.1 事务型 consumer

- 效果、事件追加、游标推进在同一个事务内。
- 提交成功即处理完成；提交前崩溃则整个事务回滚，事件重试。
- 投递本身不产生重复效果；重复的用户输入由领域幂等键处理。
- handler 必须是本地、快速、无外部 I/O 的。

### 6.2 at-least-once consumer

- handler 完成副作用后，在单独事务中推进游标。
- 两步之间崩溃会导致重复投递；consumer 必须用事件 `id` 或其他幂等键去重。
- 适合 webhook、外部消息总线、对象存储等场景。

### 6.3 顺序与失败

- 同一 partition 内严格按 `seq` 顺序；跨 partition 不保证顺序。
- handler 失败：事务回滚，游标不推进，事件重试。
- 失败按 partition 隔离，其他 partition 继续。
- dispatcher 在内存中做指数退避，避免毒事件热循环；进程重启后立即重试。
- 首期没有 dead-letter；毒事件只阻塞自己的 partition，通过日志/指标告警人工处理。暂停、跳过、dead-letter 后续再加。

### 6.4 游标推进规则

- 只推进到本次事务实际读取并处理的触发事件。
- 不能取 partition 的 `max(seq)`：并发追加的事件可能尚未被读取，取 max 会跳过它们。
- 审计事件不在订阅的 `kinds` 中，不影响游标。
- 事务型 consumer 的游标行和效果同事务提交。

## 7. 并发与锁

1. partition 游标行是序列化锁；同一 `(consumer_id, partition_key)` 只能有一个处理者。
2. 锁顺序固定：**游标 → 聚合行**。设施在调用 handler/command 之前锁游标；handler 不得先锁聚合再锁游标。
3. 同步命令路径必须调用 `process_partition`，在 `command` 闭包内锁聚合、校验、追加事件，然后处理待办事件。
4. dispatcher 和命令路径可以在同一 partition 上竞争；行锁保证串行，后到者看到更新后的游标。
5. 多 dispatcher 实例用 `FOR UPDATE SKIP LOCKED` 抢游标。
6. 跨 Task 效果只追加目标 partition 的事件，不锁目标游标或聚合；目标 dispatcher 稍后处理。
7. 不得同时持有两个 partition 游标；不得在聚合锁之后获取游标锁。
8. 大积压按批处理，避免长事务；workflow 必须快速、本地、无外部调用。

## 8. Task workflow 作为消费者

```text
consumer_id       "task-workflow"
kinds             TaskCreated, InputReceived, ActorBound,
                  ChildTerminal, DispatchRequested
partition_by      Aggregate (task_id)
delivery          Transactional
initial_position  Beginning
```

handler 在目标 Task 的行锁内：

1. 加载 `TaskDAO`、`TaskActorDAO`、依赖状态、已终止子 Task 快照；
2. 如果 Task 已经 `Completed`（待办事件在完成之后才被处理），不调用 workflow、不产生效果，只推进游标；
3. 调用 `TaskWorkflow::on_event`；
4. 应用效果，追加效果事件；
5. 推进游标。

同步命令：

- `send_input`、`bind_actor`、`unbind_actor` 追加对应事件，然后在同一事务内调用 `process_partition(task-workflow, task_id)`，使 API 可以返回处理后的结果。
- 评论、附件不进入 workflow，不使用该 consumer，也不拿游标锁。

跨 Task 触发：

| 场景 | 写入方 | 事件 | 目标 partition |
|---|---|---|---|
| 子 Task 创建 | 父 workflow 事务 | `TaskCreated` | 子 `task_id` |
| 子 Task 终止 | 子 Task 事务 | `ChildTerminal` | 父 `task_id` |
| 依赖满足 | 调度器 | `DispatchRequested` | 依赖方 `task_id` |

- 写入方只追加事件，不锁目标游标、不锁目标 Task、不修改目标状态。
- 目标 workflow 由目标 partition 的 dispatcher 在目标 Task 行锁内执行。
- 这是唯一允许的跨 Task 写入；它只表达事实，不改变目标状态。没有例外，也没有跨 Task 的强制终止。
- `Task.operations` 按 `aggregate_id` 读取 `events`，因此 `ChildTerminal`、`DispatchRequested` 会出现在目标 Task 的历史里；游标不会。

## 9. 数据模型

```sql
create table event_consumer_cursors (
    consumer_id   text not null,
    partition_key text not null,
    last_seq      bigint not null default 0,
    updated_at    timestamptz not null default now(),
    primary key (consumer_id, partition_key)
);

create index if not exists events_kind_seq_idx
    on events (kind, seq);
```

- `events` 表结构不变；已有 `(aggregate_id, seq)` 和 `(project_id, seq)` 索引。
- 不建 subscription 表；订阅是代码。
- 不建 outbox/delivery 表。
- 游标是运维数据，不通过 Task/API 暴露。
- 迁移：`0006_event_consumer_cursors.sql`；`0005_drop_outbox.sql` 已经删除 outbox。

## 10. 代码落位

- `domain`：`DomainEvent`、`DomainEventKind`。
- `app`：`EventLog`、`EventConsumer`、`Subscription`、`EventDispatcher`、`UnitOfWork` 端口。
- `adapters/postgres`：事件追加、游标表、候选分区查询、Postgres `UnitOfWork`。
- `adapters/memory`：测试用事件日志和游标。
- `services/control-plane`：dispatcher 循环、consumer 注册、配置。
- `adapters/events`：只在有外部 broker/webhook consumer 时恢复，不作为空壳。
- 现有 `EventSink` 被 `EventLog` + `UnitOfWork` 吸收；任何端口都不再自己开事务。

## 11. 运维

- 指标：每个 consumer/partition 的 lag（`max(seq) - last_seq`）、处理速率、失败次数、最老未处理事件年龄。
- 日志：`consumer_id`、`partition_key`、`seq`、事件 kind。
- 告警：lag 超阈值、同一 partition 连续失败。
- 首期不做暂停/恢复/跳过/dead-letter；设计上允许管理员重置游标或跳过毒事件，后续补运维接口。
- 新增 consumer 的回放：首期从 0 开始；从注册时开始需要持久化起始位置，见待决问题。
- 删除 consumer 的游标行手工清理。

## 12. 与 outbox 的关系

| | 已删除的 outbox | 事件总线 |
|---|---|---|
| 存储 | 每个事件一份投递行 | 事件只存一份 |
| 状态 | 投递状态（attempts/available_at/published_at） | 每个 consumer/partition 的读位置 |
| 消费者 | 不存在 | workflow、投影、通知等 |
| 定位 | 外部投递队列 | 平台事件日志 + 订阅 |

`outbox` 表已由 `0005_drop_outbox.sql` 删除，不重新引入。事件总线复用 `events`，不复制 payload。

## 13. 实施计划

### 阶段 0：确认设计

- 评审本 RFC。
- 确认订阅注册方式、游标表、锁顺序和事务边界。
- 确认首期只支持 `Beginning` 起始位置。

### 阶段 1：核心契约与内存实现

- `EventLog`、`EventConsumer`、`Subscription`、`ConsumerId`、`PartitionKey`、`DeliveryMode`、`UnitOfWork`。
- 内存事件日志和游标；一个测试 consumer。
- 覆盖顺序、分区隔离、游标推进、失败回滚。

### 阶段 2：Postgres 与 dispatcher

- `event_consumer_cursors` 迁移、`events(kind, seq)` 索引。
- Postgres `UnitOfWork`：领域写入、事件追加、游标更新同事务。
- 候选分区查询、`process_partition`、多实例 `SKIP LOCKED`。
- control-plane 后台 dispatcher、配置、指标。
- 崩溃/重试/并发测试。

### 阶段 3：Task workflow consumer

- 按 RFC 0003 的 Task 运行时实现 `task-workflow` consumer。
- 同 Task 命令同步走 `process_partition`；跨 Task 触发只追加事件。
- 子 Task 终止、调度唤醒的端到端测试。

### 阶段 4：运维与外部 consumer

- lag/失败指标、退避、告警。
- 有外部 webhook/broker 需求时增加 `adapters/events` 和 at-least-once consumer。

## 14. 验收标准

1. 同一个事件只存一行；两个 consumer 用各自游标处理，互不影响。
2. 事务型 consumer 的效果与游标同事务；提交前崩溃重试后不产生部分效果。
3. 同一 partition 严格按 `seq` 顺序；不同 partition 可并行。
4. 一个 partition 失败不影响其他 partition；游标不推进并重试。
5. 多 dispatcher 实例不会重复处理同一 partition。
6. 命令路径和 dispatcher 使用同一锁顺序；并发输入与子 Task 终止不会死锁。
7. 子 Task 终止只追加父 Task 的 `ChildTerminal`，不锁父 Task；父 workflow 由父 partition 的 dispatcher 执行。
8. 游标不会跳过处理期间并发追加的事件。
9. 游标状态不出现在 `events` 或 `Task.operations` 中。
10. 新增一个非 Task 的 consumer 不需要修改事件生产者。
11. 所有跨 Task 写入都只追加事件，不锁目标 Task、不修改目标状态；没有跨 Task 的强制终止或同步级联。

## 15. 风险与待决问题

1. **新增 consumer 的起始位置**：从头回放还是从注册时开始；后者需要持久化起始 seq。
2. **订阅变更**：改变 `kinds`/分区/投递语义时，是换 `consumer_id` 还是迁移游标；需要运维流程。
3. **毒事件**：首期只阻塞自己的 partition；暂停/跳过/dead-letter 的接口和权限待定。
4. **外部 consumer 幂等**：事件 `id` 作为幂等键的约定、重复投递窗口和重试策略。
5. **事件 schema 演进**：kind 版本化或 payload 兼容规则尚未定义。
6. **候选分区查询性能**：大规模事件下的索引、批大小、`LISTEN/NOTIFY` 优化。
7. **游标清理**：删除 consumer 后的游标行和过期 partition 的清理策略。
8. **长事务/积压**：同步命令路径处理大量待办事件时的批大小和超时策略。
9. **事件保留/归档**：事件永不删除；归档后游标落后于归档边界时的处理。
10. **租户隔离**：总线不强制项目隔离；consumer 必须自己按项目加载数据。
11. **多订阅资源隔离**：一个进程内多个 consumer 的并发、批次和故障隔离。
12. **跨 Task 没有强制操作**：事件总线只允许追加事件，父 Task 无法终止或修改子 Task。子 Task 卡住时会永久阻塞父 Task 的完成，只能由 PM/人工升级或创建替代 Task；这是 Task 模型的已知限制，不是总线要解决的问题。
