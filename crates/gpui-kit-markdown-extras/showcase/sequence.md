# 时序图

Mermaid `sequenceDiagram` 的六张示例：参与者、消息、激活框、注释与组合片段（`alt` / `else` / `opt` / `par` / `loop` / `break`），以及带起始 / 步长的 `autonumber` 与 `create` / `destroy`，都由 GPUI 原生绘制。

## 1 基础：请求与响应

两个参与者、一来一回，最小时序图。

```mermaid
sequenceDiagram
    participant C as 客户端
    participant S as 服务端
    C->>S: GET /health
    S-->>C: 200 OK
```

## 2 中等：登录校验

`autonumber` 编号、`+`/`-` 激活框、注释、`alt` / `else` 分支与 `loop` 循环。

`+` 激活箭头右侧的接收方，`-` 关闭箭头左侧的发送方，必须成对出现；也可以像末尾那样用 `activate` / `deactivate` 显式配对。

```mermaid
sequenceDiagram
    autonumber
    actor U as 用户
    participant A as 前端
    participant B as 服务
    U->>+A: 提交登录
    A->>+B: 校验凭证
    B-->>-A: 返回结果
    Note right of B: 缓存命中
    alt 校验通过
        A-->>U: 200
    else 校验失败
        A-->>U: 401
    end
    deactivate A
    loop 每 30 秒
        A-)B: 心跳
    end
```

## 3 复杂：下单支付

自发消息、`Note over`、异步消息 `-)`、失败叉号 `--x`，以及 `alt` 包裹的主成功 / 失败分支。

```mermaid
sequenceDiagram
    autonumber
    actor U as 用户
    participant W as 商城前端
    participant O as 订单服务
    participant P as 支付网关
    participant Q as 消息队列
    U->>+W: 提交订单
    W->>+O: 创建订单
    O->>O: 计算优惠与运费
    Note over O,Q: 订单落库后异步通知
    O--)Q: 发布 OrderCreated
    O-->>-W: 返回订单号
    W->>+P: 发起支付
    alt 支付成功
        P-->>W: 支付凭证
        W->>O: 回调确认
        O--)Q: 发布 OrderPaid
        W-->>U: 显示支付成功
    else 支付失败
        P--xW: 返回失败码
        W->>O: 关闭订单
        O--)Q: 发布 OrderClosed
        W-->>U: 提示重新支付
    end
    deactivate P
    deactivate W
    loop 每 30 秒
        W->>P: 轮询支付状态
        P-->>W: pending
    end
```

## 4 进阶：嵌套片段

`alt` 里嵌 `par` / `opt`，失败分支再用 `break` 提前退出，验证多层片段框与分隔线。

```mermaid
sequenceDiagram
    participant GW as 网关
    participant A as 认证
    participant B as 业务
    participant D as 数据库
    GW->>A: 校验令牌
    alt 令牌有效
        A-->>GW: 用户身份
        GW->>B: 转发请求
        par 并行读取
            B->>D: 读取账户
            B->>D: 读取额度
        and 汇总
            B->>B: 合并结果
        end
        opt 命中缓存
            B-->>GW: 直接返回
        end
    else 令牌过期
        A-->>GW: 401
        break 无法继续
            GW-->>GW: 记录审计日志
        end
    end
```

## 5 编号起始 / 步长

`autonumber 10 5` 从 10 起、每条 +5（10 / 15 / 20 / 25），适合给消息编号留出间隔或从指定值开始。

```mermaid
sequenceDiagram
    autonumber 10 5
    participant C as 客户端
    participant S as 服务端
    C->>S: 提交表单
    S-->>C: 返回回执
    S->>C: 异步通知
    C-->>S: 回执确认
```

## 6 动态创建与销毁

`create participant W as 工作进程` 让参与者中途登场，`destroy W` 标记销毁（生命线画叉，但参与者不会被移除）。

```mermaid
sequenceDiagram
    participant M as 主控
    create participant W as 工作进程
    M->>W: 派发任务
    W-->>M: 返回结果
    destroy W
```
