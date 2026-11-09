# Native diagrams

## Flowchart

```mermaid
flowchart LR
    A[Write a document] --> B{Review}
    B -->|Revise| A
    B -->|Ready| C[Publish]
```

Text after the flowchart.

## Sequence

```mermaid
sequenceDiagram
    participant Reader
    participant Editor
    Reader->>Editor: Open Markdown
    Editor-->>Reader: Render preview
    Reader->>Editor: Edit source
    Editor-->>Reader: Refresh preview
```

Text after the sequence diagram.

## Class diagram

```mermaid
classDiagram
    Document <|-- MarkdownDocument
    Document : +String text
    Document : +save()
    MarkdownDocument : +render()
```

Text after the class diagram.

## Invalid diagram

```mermaid
not a valid diagram at all
```

Invalid notation must remain readable, with an error available on hover.

[Top](#) · [Reading](typography.md)
