# 类图

Mermaid `classDiagram` 的五张示例：类成员、注解、七类关系、多重度与注释，类框高度随成员行数变化，以及自关系与 `classDef` / `cssClass` 样式指令。

## 1 基础：继承

一个父类、一个子类，最直接的继承关系。

```mermaid
classDiagram
    class Shape {
        +String color
        +area()
    }
    class Circle {
        +float radius
        +area()
    }
    Shape <|-- Circle
```

## 2 中等：关系符号集合

继承、组合、关联、依赖、实现各有不同端点符号，多重度写在两端，注释用虚线框。

```mermaid
classDiagram
    direction TB
    class Animal {
        +int age
        -String name
        +void eat()
    }
    <<interface>> Animal
    Animal <|-- Duck
    Animal *-- Heart : 组合
    Duck "1" --> "*" Pond : 关联
    Duck ..> Food : 依赖
    Duck ..|> Swim
    note for Duck "水禽"
```

## 3 复杂：三层服务架构

横向排布的多层调用链，接口注解写在类块内，仓库与实体之间用依赖虚线。

```mermaid
classDiagram
    direction LR
    class OrderController {
        -OrderService service
        +create(request)
        +cancel(id)
    }
    class OrderService {
        -OrderRepository repo
        -PaymentClient pay
        +submit(order)
        +cancel(id)
    }
    class OrderRepository {
        <<interface>>
        +save(order)
        +findById(id)
    }
    class PaymentClient {
        +charge(orderId, amount)
        +refund(orderId)
    }
    class Order {
        +String id
        +List items
        +Money total
        +pay()
    }
    class OrderItem {
        +String sku
        +int qty
    }
    OrderController --> OrderService : 调用
    OrderService --> OrderRepository : 读写
    OrderService --> PaymentClient : 支付
    OrderRepository ..> Order : 持久化
    Order "1" *-- "1..*" OrderItem : 包含
    note for PaymentClient "外部网关，超时重试"
```

## 4 进阶：泛型与聚合

泛型类名用 `~` 标注（ID 取 `~` 前的部分），实现关系与聚合空心菱形。

```mermaid
classDiagram
    class Repository~T~ {
        <<interface>>
        +get(id) T
        +save(entity T)
    }
    class UserRepository {
        +get(id) User
        +save(user)
    }
    class Cache~K, V~ {
        +put(key K, value V)
        +get(key K) V
    }
    class AuditLog {
        +append(event)
    }
    Repository~User~ <|.. UserRepository
    UserRepository o-- Cache : 二级缓存
    UserRepository ..> AuditLog : 记录
    Cache "1" --> "*" Entry : 持有
```

## 5 自关系与样式指令

`Worker --> Worker` 是自关系，回环画在类框侧面而不穿过别的框；`classDef` / `cssClass` 是样式指令，解析时直接跳过，不会误生成名为「classDef …」的类。

```mermaid
classDiagram
    class Worker {
        +String name
        +run()
        +retry()
    }
    class Queue {
        +push(job)
        +pop()
    }
    Worker --> Queue : 投递任务
    Worker --> Worker : 失败自重试
    classDef highlight fill:#f9f,stroke:#333
    cssClass Worker highlight
```
