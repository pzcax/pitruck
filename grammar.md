# Pitruck Grammar

## Top level

$$
\begin{align}
\langle\text{program}\rangle &::= \langle\text{stmt}\rangle^*
\end{align}
$$

## Statements

$$
\begin{align}
\langle\text{stmt}\rangle &::= \texttt{var}\ \text{IDENT}\ \texttt{=}\ \langle\text{expr}\rangle\ \langle\text{stmt-end}\rangle \\
  &\mid \texttt{bring}\ \text{IDENT}\ \langle\text{stmt-end}\rangle \\
  &\mid \texttt{func}\ \text{IDENT}\ \texttt{(}\ \langle\text{params}\rangle\ \texttt{)}\ \langle\text{block}\rangle \\
  &\mid \texttt{class}\ \text{IDENT}\ \langle\text{block}\rangle \\
  &\mid \texttt{match}\ \langle\text{expr}\rangle\ \texttt{\{}\ \langle\text{match-arm}\rangle^*\ \texttt{\}} \\
  &\mid \texttt{return}\ \langle\text{expr}\rangle^?\ \langle\text{stmt-end}\rangle \\
  &\mid \texttt{print}\ \langle\text{expr}\rangle\ \langle\text{stmt-end}\rangle \\
  &\mid \langle\text{if-stmt}\rangle \\
  &\mid \texttt{while}\ \langle\text{expr}\rangle\ \langle\text{block}\rangle \\
  &\mid \langle\text{lvalue}\rangle\ \texttt{=}\ \langle\text{expr}\rangle\ \langle\text{stmt-end}\rangle \quad \text{(assignment)} \\
  &\mid \langle\text{expr}\rangle\ \langle\text{stmt-end}\rangle \quad \text{(expr-stmt)}
\end{align}
$$

## If statement

$$
\begin{align}
\langle\text{if-stmt}\rangle    &::= \texttt{if}\ \langle\text{expr}\rangle\ \langle\text{block}\rangle\ \langle\text{elif-clause}\rangle^*\ \langle\text{else-clause}\rangle^? \\
\langle\text{elif-clause}\rangle &::= \texttt{elif}\ \langle\text{expr}\rangle\ \langle\text{block}\rangle \\
\langle\text{else-clause}\rangle &::= \texttt{else}\ \langle\text{block}\rangle
\end{align}
$$

## Match arms

$$
\begin{align}
\langle\text{match-arm}\rangle &::= \langle\text{expr}\rangle\ \texttt{=>}\ \langle\text{arm-body}\rangle\ \texttt{,}^? \\
  &\mid \texttt{\_}\ \texttt{=>}\ \langle\text{arm-body}\rangle\ \texttt{,}^? \quad \text{(default, at most one)} \\\\
\langle\text{arm-body}\rangle &::= \langle\text{block}\rangle \\
  &\mid \langle\text{stmt}\rangle
\end{align}
$$

## Block & statement terminator

$$
\begin{align}
\langle\text{block}\rangle &::= \texttt{\{}\ \langle\text{stmt}\rangle^*\ \texttt{\}} \\
  &\mid \text{NEWLINE}\ \text{INDENT}\ \langle\text{stmt}\rangle^*\ \text{DEDENT} \\\\
\langle\text{stmt-end}\rangle &::= \text{NEWLINE}^+ \mid \text{DEDENT} \mid \text{EOF} \mid \texttt{\}} \mid \varepsilon
\end{align}
$$

## Assignment targets (lvalue)

$$
\begin{align}
\langle\text{lvalue}\rangle &::= \text{IDENT} \\
  &\mid \langle\text{expr}\rangle\ \texttt{.}\ \text{IDENT} \quad \text{(field set)} \\
  &\mid \langle\text{expr}\rangle\ \texttt{[}\ \langle\text{expr}\rangle\ \texttt{]} \quad \text{(index set)}
\end{align}
$$

## Expressions — precedence (low → high)

$$
\begin{align}
\langle\text{expr}\rangle        &::= \langle\text{or-expr}\rangle \\\\
\langle\text{or-expr}\rangle     &::= \langle\text{and-expr}\rangle\ (\texttt{or}\ \langle\text{and-expr}\rangle)^* \\\\
\langle\text{and-expr}\rangle    &::= \langle\text{not-expr}\rangle\ (\texttt{and}\ \langle\text{not-expr}\rangle)^* \\\\
\langle\text{not-expr}\rangle    &::= \texttt{not}\ \langle\text{not-expr}\rangle \\
  &\mid \langle\text{cmp-expr}\rangle \\\\
\langle\text{cmp-expr}\rangle    &::= \langle\text{add-expr}\rangle\ (\langle\text{cmp-op}\rangle\ \langle\text{add-expr}\rangle)^* \\\\
\langle\text{cmp-op}\rangle      &::= \texttt{==} \mid \texttt{!=} \mid \texttt{<} \mid \texttt{>} \mid \texttt{<=} \mid \texttt{>=} \\\\
\langle\text{add-expr}\rangle    &::= \langle\text{mul-expr}\rangle\ ((\texttt{+} \mid \texttt{-})\ \langle\text{mul-expr}\rangle)^* \\\\
\langle\text{mul-expr}\rangle    &::= \langle\text{unary-expr}\rangle\ ((\texttt{*} \mid \texttt{/} \mid \texttt{\%})\ \langle\text{unary-expr}\rangle)^* \\\\
\langle\text{unary-expr}\rangle  &::= \texttt{-}\ \langle\text{unary-expr}\rangle \\
  &\mid \langle\text{postfix-expr}\rangle \\\\
\langle\text{postfix-expr}\rangle &::= \langle\text{primary}\rangle\ \langle\text{postfix-op}\rangle^* \\\\
\langle\text{postfix-op}\rangle  &::= \texttt{(}\ \langle\text{args}\rangle\ \texttt{)} \quad \text{(call)} \\
  &\mid \texttt{[}\ \langle\text{expr}\rangle\ \texttt{]} \quad \text{(index get)} \\
  &\mid \texttt{.}\ \text{IDENT} \quad \text{(field get)}
\end{align}
$$

## Primary expressions

$$
\begin{align}
\langle\text{primary}\rangle &::= \text{NUMBER} \mid \text{STRING} \mid \texttt{true} \mid \texttt{false} \mid \texttt{null} \mid \texttt{self} \\
  &\mid \text{IDENT} \\
  &\mid \texttt{(}\ \langle\text{expr}\rangle\ \texttt{)} \\
  &\mid \texttt{[}\ \langle\text{args}\rangle\ \texttt{]} \quad \text{(list literal)} \\
  &\mid \texttt{\{}\ \langle\text{kv-pair}\rangle\ (\texttt{,}\ \langle\text{kv-pair}\rangle)^*\ \texttt{,}^?\ \texttt{\}} \quad \text{(dict literal)} \\
  &\mid \texttt{(}\ \langle\text{params}\rangle\ \texttt{)}\ \texttt{=>}\ \langle\text{lambda-body}\rangle \quad \text{(lambda)} \\\\
\langle\text{kv-pair}\rangle     &::= \langle\text{expr}\rangle\ \texttt{:}\ \langle\text{expr}\rangle \\\\
\langle\text{lambda-body}\rangle &::= \langle\text{block}\rangle \mid \langle\text{expr}\rangle \quad \text{(implicit return)}
\end{align}
$$

## Shared productions

$$
\begin{align}
\langle\text{params}\rangle &::= \varepsilon \mid \text{IDENT}\ (\texttt{,}\ \text{IDENT})^* \\\\
\langle\text{args}\rangle   &::= \varepsilon \mid \langle\text{expr}\rangle\ (\texttt{,}\ \langle\text{expr}\rangle)^*\ \texttt{,}^?
\end{align}
$$