# 甘特图

Mermaid `gantt` 的四张示例：`done` / `active` / `crit` 用主题语义色区分，里程碑画菱形，`excludes weekends` 会把时间轴压成工作日。

## 1 基础：小型迭代

单 section，显式日期与时长，三种任务状态各一条。

```mermaid
gantt
    title 小型迭代
    dateFormat YYYY-MM-DD
    axisFormat %m/%d
    todayMarker off
    section 开发
    需求梳理 :done, t1, 2026-02-02, 2d
    编码实现 :active, t2, 2026-02-04, 5d
    自测修复 : t3, 2026-02-09, 2d
```

## 2 中等：多段计划与里程碑

两个 section，用 `after` 串联依赖，最后以里程碑收尾。

```mermaid
gantt
    title 版本 0.2 计划
    dateFormat YYYY-MM-DD
    axisFormat %m/%d
    tickInterval 1week
    excludes weekends
    todayMarker off
    section 设计
    需求分析 :done, des1, 2026-01-05, 3d
    原型评审 :active, des2, after des1, 2d
    section 开发
    编码 :crit, dev1, after des2, 10d
    联调 : dev2, after dev1, 5d
    发版 :milestone, m1, after dev2, 0d
```

## 3 复杂：工作日排期

`excludes weekends` 去掉周末，`crit` 标出关键路径，联调依赖两个前序任务。

```mermaid
gantt
    title 发布计划（工作日轴）
    dateFormat YYYY-MM-DD
    axisFormat %m/%d
    tickInterval 3d
    excludes weekends
    todayMarker off
    section 准备
    冻结需求 :done, p1, 2026-03-02, 3d
    风险评估 :done, p2, after p1, 2d
    section 开发
    核心功能 :crit, d1, after p2, 10d
    外围功能 : d2, after p2, 8d
    联调 : d3, after d1 d2, 4d
    section 验收
    回归测试 : a1, after d3, 5d
    灰度发布 : a2, after a1, 3d
    正式发布 :milestone, a3, after a2, 0d
```

## 4 进阶：今日标记

不关 `todayMarker`，时间轴落在本月，竖线标出当天；进行中的任务用 `active` 高亮。

```mermaid
gantt
    title 本月冲刺
    dateFormat YYYY-MM-DD
    axisFormat %m/%d
    tickInterval 2d
    section 当前
    已完成 :done, s1, 2026-11-02, 3d
    进行中 :active, s2, 2026-11-05, 4d
    待开始 : s3, after s2, 4d
    section 后续
    验收 :crit, s4, after s3, 2d
    里程碑 :milestone, s5, after s4, 0d
```
