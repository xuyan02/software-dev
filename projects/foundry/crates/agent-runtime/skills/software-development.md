# 软件开发流程技能

本技能覆盖 `software_development` 根流程和 `daily_check` 日常检查任务。

## software_development

根流程按阶段创建需求、设计、开发、测试、发布和反馈子任务。默认“日常检查”触发器由控制面在项目创建/启动时确定性创建，不再由智能体执行初始化。

## daily_check

日常检查任务由触发器创建，工作流本身在所有检查完成后以 `completed` 结束：

1. `audits` 阶段一次性并行创建五个子任务：`requirements_audit`、`design_audit`、`code_audit`、`test_audit`、`product_audit`；
2. 五个子任务完成后进入 `synthesis` 阶段，汇总智能体调用 `task_children` 读取所有子任务评论；
3. 汇总智能体去重、归并并把发现分流到设计池/开发池，然后发送 `synthesis:synthesized`。

日常检查本身不直接修改代码或设计；检查由各审计子任务的专门智能体完成，发现的问题通过 Issue 进入分诊。
