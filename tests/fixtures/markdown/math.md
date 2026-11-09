# Mathematics $x^2$

Text above the inline equations.

Inline $E = mc^2$, a fraction $\frac{a_i}{b_j}$, a sum $\sum_{i=1}^{n} x_i$, and a matrix $\begin{bmatrix} a & b \\ c & d \end{bmatrix}$ belong with the surrounding text.

Text below the inline equations.

## Display equations

$$
\int_0^1 x^2\,dx = \frac{1}{3}
$$

The paragraph after an integral must be separate.

```math
\begin{aligned}
a^2 + b^2 &= c^2 \\
e^{i\pi} + 1 &= 0
\end{aligned}
```

The paragraph after aligned equations must be separate.

## Nested equations

- A list fraction $\frac{1}{1 + x^2}$ beside prose.
- A second item follows it.

> A quoted matrix $\begin{pmatrix} 1 & 0 \\ 0 & 1 \end{pmatrix}$ beside prose.
>
> A paragraph below the matrix.

| Notation | Explanation |
| --- | --- |
| $\frac{a}{b}$ | A quotient |
| $\sum_{i=1}^{n} x_i$ | A sum |

## Wide equation

$$
x_1 + x_2 + x_3 + x_4 + x_5 + x_6 + x_7 + x_8 + x_9 + x_{10} + x_{11} + x_{12} + x_{13} + x_{14} + x_{15} + x_{16} + x_{17} + x_{18} + x_{19} + x_{20} = y
$$

The wide equation must scroll without shrinking this paragraph.

## Invalid notation

Malformed $\notARealCommand{broken}$ must remain readable as source.

Literal prices use escaped dollars: \$12.50 and \$20.

[Top](#) · [Reading](typography.md)
