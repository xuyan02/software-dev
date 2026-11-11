# Document tool

Use the single `document` tool for all document work. Select the operation with `action`.

- `toc`: read metadata and the Markdown table of contents.
- `draft_open` / `draft_create`: open or create the shared draft.
- `content`: read Markdown content.
- `add` / `update_text` / `delete` / `move`: edit the draft.
- `validate`: check the draft before submitting.
- `annotations_list` / `annotate` / `resolve_annotation`: read and write annotations.
- `approvals` / `versions`: inspect approval and version history.
- `decide`: approve or reject a draft revision. Only humans may use this action.

Single chapters are pre-created. Fill them with `update_text`. Use `add` only for multiple
chapter instances and list items. Single chapters cannot be deleted.
