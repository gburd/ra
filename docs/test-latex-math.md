# LaTeX Math Test

This page tests LaTeX/MathJax rendering in VitePress.

## Inline Math

Selection operator: $\sigma_{p}(R)$

Projection operator: $\pi_{A_1, A_2, ..., A_n}(R)$

Join operator: $R \bowtie_{p} S$

## Block Math

The selection operator filters rows based on a predicate:

$$
\sigma_{p}(R) = \{t \in R \mid p(t)\}
$$

The projection operator selects specific columns:

$$
\pi_{A_1, A_2, ..., A_n}(R) = \{t[A_1, A_2, ..., A_n] \mid t \in R\}
$$

## Complex Equation

Cost model for a query plan:

$$
\begin{align}
\text{Cost}(Plan) &= \sum_{i=1}^{n} \text{Cost}(Op_i) \\
&= C_{scan} + C_{join} + C_{project} + C_{aggregate}
\end{align}
$$
