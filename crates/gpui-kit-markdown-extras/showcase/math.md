# 公式

`gpui-kit-markdown-extras` 的公式渲染验证页：行内公式、描述统计、相关与回归、校准曲线与常见数学记号，全部在 GPUI 里原生绘制。

## 1 行内公式

质能方程 $E = mc^2$；勾股定理 $a^2 + b^2 = c^2$；圆周率 $\pi \approx 3.1416$。

样本量 $n$ 与第 $i$ 个观测值 $x_i$ 构成序列 $x_1, x_2, \ldots, x_n$；当 $n \to \infty$ 时，$\bar{x}$ 依概率收敛到 $\mu$。

判断条件：$1 \le i \le n$ 且 $i \ne j$；误差范围 $\pm 5\%$；相关系数 $r \ge 0.99$ 视为线性良好。

## 2 描述统计

均值、样本标准差与变异系数：

$$\bar{x} = \frac{1}{n}\sum_{i=1}^{n} x_i$$

$$s = \sqrt{\frac{\sum_{i=1}^{n}\left(x_i - \bar{x}\right)^2}{n - 1}}$$

$$CV = \frac{s}{\bar{x}} \times 100\%$$

偏倚（相对偏差）：

$$Bias\% = \frac{\bar{x} - \mu}{\mu} \times 100\%$$

## 3 相关与回归

皮尔逊相关系数——长分式里套两个求和与一个双层根号：

$$r = \frac{\sum_{i=1}^{n}\left(x_i - \bar{x}\right)\left(y_i - \bar{y}\right)}{\sqrt{\sum_{i=1}^{n}\left(x_i - \bar{x}\right)^2 \sum_{i=1}^{n}\left(y_i - \bar{y}\right)^2}}$$

最小二乘直线 $y = ax + b$，其中斜率与截距为：

$$a = \frac{\sum\left(x_i - \bar{x}\right)\left(y_i - \bar{y}\right)}{\sum\left(x_i - \bar{x}\right)^2}, \quad b = \bar{y} - a\bar{x}$$

## 4 校准曲线

四参数 Logistic（4PL）曲线：

$$y = \frac{A - D}{1 + \left(x / C\right)^{B}} + D$$

切线近似与稀释回收率：

$$y \approx y_0 + k\left(x - x_0\right), \quad R = \frac{c_{\text{实测}}}{c_{\text{理论}}} \times 100\%$$

## 5 数学记号

二次方程求根（覆盖 `\frac` + `\sqrt` + 上标）：

$$x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}$$

嵌套分式与嵌套根号：

$$\frac{\frac{a}{b}}{\frac{c}{d}} \quad \text{与} \quad \sqrt{1 + \sqrt{1 + \sqrt{1 + x}}}$$

求和、连乘、定积分：

$$\sum_{i=1}^{n} x_i \quad \prod_{k=1}^{m}\left(1 + \lambda_k\right) \quad \int_{0}^{1} f(x)\,dx$$

一维波动方程（偏导数与拉普拉斯算子）：

$$\frac{\partial^2 u}{\partial t^2} = c^2 \nabla^2 u$$

集合与逻辑：

$$A \subseteq B \Rightarrow \forall x \in A,\; x \in B, \quad A \cup B \ne \emptyset$$

希腊字母与常用符号：

$$\alpha + \beta = \gamma, \quad \mu \pm \sigma, \quad \Delta \approx 0, \quad \Omega \to \infty, \quad \hat{y} \pm \tilde{\varepsilon}$$

## 6 表格中的公式

| 指标 | 公式 | 判定线 |
|---|---|---|
| 均值 | $\bar{x} = \frac{1}{n}\sum x_i$ | — |
| 标准差 | $s = \sqrt{\frac{\sum (x_i-\bar{x})^2}{n-1}}$ | 越小越稳 |
| 变异系数 | $CV = \frac{s}{\bar{x}} \times 100\%$ | $CV \le 15\%$ |
| 相关系数 | $r$ | $r \ge 0.99$ |
| 偏倚 | $Bias\% = \frac{\bar{x}-\mu}{\mu} \times 100\%$ | 绝对值 ≤ 10% |

## 7 矩阵与扩充记号

矩阵环境（`pmatrix` / `bmatrix` / `vmatrix`）与分段函数 `cases`：

$$A = \begin{pmatrix} 1 & 2 \\ 3 & 4 \end{pmatrix}, \quad B = \begin{bmatrix} a & b \\ c & d \end{bmatrix}, \quad \det A = \begin{vmatrix} 1 & 2 \\ 3 & 4 \end{vmatrix}$$

$$f(x) = \begin{cases} x^2 & x \ge 0 \\ -x & x < 0 \end{cases}$$

带根指数的根号、堆叠上下标与二项式系数：

$$\sqrt[3]{x^2 + 1}, \quad x_i^2, \quad \binom{n}{k} = \frac{n!}{k!(n-k)!}$$

黑板粗体 / 花体 / 哥特体，以及 boxed、underline 与扩充符号：

$$\mathbb{R}, \quad \mathbb{N} \subseteq \mathbb{Z}, \quad \mathcal{L}\{f\}, \quad \mathfrak{g}$$

$$\boxed{E = mc^2}, \quad \underline{a + b}, \quad \langle x, y \rangle, \quad A \oplus B, \quad p \land q \lor \neg r, \quad x \mapsto y \iff z$$

## 8 对齐环境与堆叠记号

花括号 / 双竖线矩阵（`Bmatrix` / `Vmatrix`）：

$$\begin{Bmatrix} 1 & 2 \\ 3 & 4 \end{Bmatrix}, \quad \begin{Vmatrix} a & b \\ c & d \end{Vmatrix}$$

多行对齐与聚集（`aligned` / `gathered`，不带定界符）：

$$\begin{aligned} (a+b)^2 &= a^2 + 2ab + b^2 \\ (a-b)^2 &= a^2 - 2ab + b^2 \end{aligned}$$

$$\begin{gathered} x = 1 \\ y = 2 \end{gathered}$$

上下堆叠（`overset` / `underset` / `stackrel`）、上划线与编号：

$$\overset{\text{def}}{=} \quad \underset{n\to\infty}{\lim}\, a_n \quad \stackrel{!}{=} \quad \overline{z} \quad \tag{8}$$
