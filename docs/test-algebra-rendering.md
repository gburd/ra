# Algebra Rendering Test

This page demonstrates all the ways relational algebra notation
renders in the RA documentation.

## KaTeX Math Expressions

Standard LaTeX math via KaTeX:

| Operator | KaTeX Source | Rendered |
|----------|-------------|----------|
| Selection | `$\sigma_{p}(R)$` | $\sigma_{p}(R)$ |
| Projection | `$\pi_{a,b}(R)$` | $\pi_{a,b}(R)$ |
| Rename | `$\rho_{S}(R)$` | $\rho_{S}(R)$ |
| Join | `$R \bowtie S$` | $R \bowtie S$ |
| Conditional join | `$R \bowtie_{c} S$` | $R \bowtie_{c} S$ |
| Semijoin | `$R \ltimes S$` | $R \ltimes S$ |
| Antijoin | `$R \rhd S$` | $R \rhd S$ |
| Union | `$R \cup S$` | $R \cup S$ |
| Intersection | `$R \cap S$` | $R \cap S$ |
| Difference | `$R - S$` | $R - S$ |
| Product | `$R \times S$` | $R \times S$ |
| Aggregation | `$\gamma_{G;\text{sum}(A)}(R)$` | $\gamma_{G;\text{sum}(A)}(R)$ |

### Block Math

$$\sigma_{p}(R \bowtie_{c} S) \Rightarrow \sigma_{p}(R) \bowtie_{c} S$$

$$\pi_{a,b}\left(\sigma_{x > 10}(R) \bowtie S\right)$$

## Inline Algebra Plugin

The `{{...}}` syntax auto-converts text notation to Unicode symbols:

| Text Notation | Rendered |
|---------------|----------|
| `sigma[p](R)` | {{sigma[p](R)}} |
| `pi[a,b](R)` | {{pi[a,b](R)}} |
| `rho[S](R)` | {{rho[S](R)}} |
| `R join S` | {{R join S}} |
| `R join[c] S` | {{R join[c] S}} |
| `R semijoin S` | {{R semijoin S}} |
| `R antijoin S` | {{R antijoin S}} |
| `R leftjoin S` | {{R leftjoin S}} |
| `R rightjoin S` | {{R rightjoin S}} |
| `R fulljoin S` | {{R fulljoin S}} |
| `R union S` | {{R union S}} |
| `R intersect S` | {{R intersect S}} |
| `R except S` | {{R except S}} |
| `R cross S` | {{R cross S}} |
| `gamma[G; sum(A)](R)` | {{gamma[G; sum(A)](R)}} |

### Complex Expressions

Predicate pushdown:
{{sigma[p](R join[c] S)}} becomes {{sigma[p](R) join[c] S}}

Nested operations:
{{pi[a,b](sigma[x > 10](R) join S)}}

## Vue Component

The `<RelAlgebra>` component renders with hover tooltips on each
operator symbol:

<RelAlgebra expr="sigma[p](R)" />

<RelAlgebra expr="pi[a,b](R)" />

<RelAlgebra expr="R join[c] S" />

<RelAlgebra expr="R semijoin S" />

<RelAlgebra expr="gamma[G; count(A)](R)" />

## Backslash Syntax

The `\ra{...}` syntax also works inline: \ra{sigma[p](R join S)}

## Comparison

All three methods side by side for the same expression:

| Method | Output |
|--------|--------|
| KaTeX | $\sigma_{p}(R \bowtie_{c} S)$ |
| Plugin `{{...}}` | {{sigma[p](R join[c] S)}} |
| Component | <RelAlgebra expr="sigma[p](R join[c] S)" /> |

## Operator Reference

| Symbol | Unicode | Name | Prefix Syntax | Infix Syntax |
|--------|---------|------|---------------|--------------|
| $\sigma$ | U+03C3 | Selection | `sigma[p](R)` | -- |
| $\pi$ | U+03C0 | Projection | `pi[a,b](R)` | -- |
| $\rho$ | U+03C1 | Rename | `rho[S](R)` | -- |
| $\gamma$ | U+03B3 | Aggregation | `gamma[G;f(A)](R)` | -- |
| $\delta$ | U+03B4 | Duplicate Elim. | `delta(R)` | -- |
| $\tau$ | U+03C4 | Sort | `tau[a](R)` | -- |
| $\bowtie$ | U+22C8 | Join | -- | `R join S` |
| $\ltimes$ | U+22C9 | Semijoin | -- | `R semijoin S` |
| $\rhd$ | U+22CA | Antijoin | -- | `R antijoin S` |
| $\cup$ | U+222A | Union | -- | `R union S` |
| $\cap$ | U+2229 | Intersection | -- | `R intersect S` |
| $-$ | U+2212 | Difference | -- | `R except S` |
| $\times$ | U+00D7 | Product | -- | `R cross S` |
