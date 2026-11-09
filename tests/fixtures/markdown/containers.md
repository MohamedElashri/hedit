# Nested containers

1. A numbered item with **emphasis**.
   - A nested bullet with a [reference](https://example.com).
   - Another nested bullet.
2. A second numbered item.

- A loose list starts here.

  A second paragraph belongs to the first item.

  > A quote inside the first item.
  >
  > - A nested list in the quote.
  > - Another nested item.

- The next loose item must not overlap the quote.

- [x] A completed task.
- [ ] An incomplete task.

## Quotes and alerts

> A quote with a paragraph.
>
> > A nested quote with **strong words**.
>
> | Quoted header | Quoted value |
> | --- | --- |
> | Wrapped text | A sufficiently long description that wraps in a small window without escaping the surrounding quote. |
>
> ```rust
> let quoted = true;
> ```

Text outside all quotes.

> [!NOTE]
> A note should retain its text and nesting.

## Footnotes and definitions

A sentence with a footnote[^one] and another reference[^two].

[^one]: The first footnote has **formatting**.
[^two]: The second footnote has a [link](typography.md).

Term one
: A definition with several words and **strong text**.

Term two
: Another definition.

Final paragraph after every container.
