# mdpeek

Progressive-disclosure CLI for markdown/text docs in a monorepo.
Designed for LLM consumption — non-interactive, structured I/O.

## Installation

```bash
cargo install --path .
```

### Kiro CLI setup

Generate a steering file so the Kiro CLI knows how to use mdpeek:

```bash
mdpeek --llm-help > ~/.kiro/steering/mdpeek.md
```

## Quick start

```bash
mdpeek index                          # list all docs with IDs + token counts
mdpeek outline <doc-id>               # headings + snippets for one doc
mdpeek outline docs/guide.md          # also accepts file paths
mdpeek show <doc-id>                  # full doc content
mdpeek show <doc-id>:<section-id>     # single section slice
mdpeek show README.md                 # show by file path
mdpeek search "query"                 # full-text search
mdpeek search "query" --snippet       # include query-relevant excerpts
mdpeek search "query" --include-content  # embed full section content
```

Add `--json` to any command for structured output.

## Three layers

1. **Index** — discover what docs exist and their retrieval cost (token counts)
2. **Outline** — drill into a doc's heading structure with first-sentence snippets
3. **Show** — read the full doc or just the section you need

## IDs

- **Doc ID**: 8-hex SHA-256 of repo-relative path (stable across edits, changes on rename)
- **Section ID**: 6-hex SHA-256 of section content (stable across moves, changes on edit)
- Both support unambiguous prefix matching

## Ignore patterns

```bash
mdpeek ignore 'drafts/**'             # add gitignore-style patterns
mdpeek ignore --list                  # show current patterns
mdpeek ignore --remove 'drafts/**'    # remove one
mdpeek ignore --clear                 # clear all
```

## LLM integration

Run `mdpeek --llm-help` for a single comprehensive reference suitable
for inclusion in an LLM system prompt or tool description.

## Cache

Per-repo state stored at `$XDG_CACHE_HOME/mdpeek/repos/<repo-hash>/`.
Auto-indexing runs before each command with a 5-second TTL.
Use `--no-auto-index` to skip, or `mdpeek index` to force a full rescan.
