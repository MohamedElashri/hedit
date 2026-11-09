# Code and literal content

Text above the code.

```rust
fn main() {
    let café = "λ → Markdown";
    println!("{café}");
}
```

```python
def square(value: int) -> int:
    return value ** 2
```

```json
{"name": "Markdown", "enabled": true, "count": 42}
```

```diff
- old text
+ new text
```

```unknown-language
**This stays literal.** $x^2$ [not a link](#)
```

    Indented code also stays literal.
    A second indented line.

## Wide code

```text
This_is_one_very_long_unbroken_line_0123456789_abcdefghijklmnopqrstuvwxyz_ABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789_abcdefghijklmnopqrstuvwxyz_ABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789_abcdefghijklmnopqrstuvwxyz_ABCDEFGHIJKLMNOPQRSTUVWXYZ
```

Text after the wide code must remain inside the reading column.

[Top](#) · [Reading](typography.md)
