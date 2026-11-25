概念空间：

  concept/domain        现实概念
  concept/requirement   需求概念
  concept/technical     技术概念

目录：

  arche concept mkdir concept/domain/agent

概念：

  arche concept create concept/domain/agent \
    --name "智能体" \
    --explanation "能够感知环境并采取行动的系统"

字段：

  name            qname 的本地 key，如 agent
  display_name    展示名称，如“智能体”
  explanation     DocumentParagraphList

同一个 key 可以同时存在于三个目录：

  concept/domain/agent
  concept/requirement/agent
  concept/technical/agent

它们是三个独立对象。

定义与判断：

  概念是含义不显然、容易被不同读者理解成不同东西、或不同文档用法不一致的术语。
  正例：智能体能感知环境并采取行动。
  反例：为“数据”“工具”这类日常词建概念，没有定义价值。
  概念写定义、边界和关系，不写实现细节。
