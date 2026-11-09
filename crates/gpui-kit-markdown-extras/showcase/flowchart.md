# 流程图

Mermaid `flowchart` 的三张示例，由浅入深覆盖分层、扇出、扇入、回边、菱形判断与边标注，全部由 GPUI 原生绘制。

## 1 发布流水线（扇出 / 扇入 / 三段回边）

从静态检查一路到更新清单：失败分支回到「通知提交者」形成扇入，修复后回到起点形成外层循环，覆盖率不足回到单元测试形成内层循环。

```mermaid
flowchart TD
    A[提交代码] --> B{静态检查}
    B -- 通过 --> C[编译构建]
    B -- 失败 --> D[通知提交者]
    D --> E[本地修复]
    E --> A
    C --> F{单元测试}
    F -- 通过 --> G[集成测试]
    F -- 失败 --> D
    G --> H{覆盖率达标}
    H -- 是 --> I[生成安装包]
    H -- 否 --> J[补充用例]
    J --> F
    I --> K[签名与校验]
    K --> L{灰度发布}
    L -- 通过 --> M[全量发布]
    L -- 异常 --> N[自动回滚]
    N --> D
    M --> O[写入更新清单]
```

## 2 订单履约（多路汇聚）

下单后按库存分两条路，支付失败要释放库存并关单；签收不通过会回到「签收」重新判定；缺货单与关单最后都汇入订单完成。

```mermaid
flowchart TD
    Start[创建订单] --> Stock{库存校验}
    Stock -- 有货 --> Lock[锁定库存]
    Stock -- 缺货 --> Backorder[生成缺货单]
    Backorder --> Notify[通知客户]
    Lock --> Pay{支付结果}
    Pay -- 成功 --> Ship[创建发货单]
    Pay -- 失败 --> Unlock[释放库存]
    Unlock --> Close[关闭订单]
    Ship --> Deliver[物流揽收]
    Deliver --> Signed{客户签收}
    Signed -- 是 --> Settle[结算对账]
    Signed -- 否 --> Redeliver[二次派送]
    Redeliver --> Signed
    Settle --> Done[订单完成]
    Notify --> Done
    Close --> Done
```

## 3 日志告警（长回退边 + 虚线）

采集、解析、聚合、检测、去重、分级通知；死信人工重放回到解析，重复告警直接落库形成长回退边。

```mermaid
flowchart TD
    Ingest[采集日志] --> Parse{解析成功}
    Parse -- 是 --> Enrich[补齐维度]
    Parse -- 否 --> Dead[进入死信队列]
    Enrich --> Aggregate[按分钟聚合]
    Aggregate --> Detect{异常检测}
    Detect -- 正常 --> Store[写入冷存储]
    Detect -- 异常 --> Dedup{是否已告警}
    Dedup -- 新告警 --> Notify[通知值班]
    Dedup -- 已知 --> Suppress[抑制重复]
    Notify --> Escalate{需要升级}
    Escalate -- 是 --> Oncall[呼叫二线]
    Escalate -- 否 --> Ticket[生成工单]
    Oncall --> Resolve[处理完成]
    Ticket --> Resolve
    Suppress --> Store
    Dead --> Replay[人工重放]
    Replay -.-> Parse
```
