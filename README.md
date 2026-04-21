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
Use `-C <path>` to run against a different directory (like `git -C`).

## Example

Running against a checkout of [kubernetes/kubernetes] (294 docs, ~4M tokens):

```console
$ mdpeek index
./
| ID       | File                 | Title                          | Tokens | Modified   |
|----------|----------------------|--------------------------------|--------|------------|
| eca12c0a | CONTRIBUTING.md      | Contributing                   |    122 | 2026-03-06 |
| b3356305 | README.md            | Kubernetes (K8s)               |    954 | 2026-03-06 |
| c5fe610b | SUPPORT.md           | Support for deploying and usi… |    238 | 2026-03-06 |
...
CHANGELOG/
| ID       | File                 | Title                          | Tokens | Modified   |
|----------|----------------------|--------------------------------|--------|------------|
| 549522f6 | CHANGELOG-1.10.md    | v1.10.13                       | 106590 | 2026-03-06 |
| 20755dcf | CHANGELOG-1.11.md    | v1.11.10                       | 101469 | 2026-03-06 |
...
```

Drill into a doc's structure before reading it:

```console
$ mdpeek outline README.md
README.md  [b3356305]  954 tokens

# Kubernetes (K8s)  [sec:7e4af3]  (274 tokens)
[![CII Best Practices]...

## To start using K8s  [sec:9c9999]  (92 tokens)
See our documentation on [kubernetes.io].

## To start developing K8s  [sec:690906]  (52 tokens)
The [community repository] hosts all information about building Kubernetes...
...
```

Read just the section you want:

```console
$ mdpeek show README.md:9c9999
## To start using K8s

See our documentation on [kubernetes.io].

Take a free course on [Scalable Microservices with Kubernetes].
...
```

Search across the whole corpus:

```console
$ mdpeek search "kubectl install" --limit 3 --snippet
| Doc      | Sec    | Path                           | Heading            | Tokens | Score |
|----------|--------|--------------------------------|--------------------|--------|-------|
| 452a5c75 | 0a4365 | hack/tools/golangci-lint/sort… | Installation       |      3 | 39.02 |
  ## Installation
| 51671a26 | 2e9849 | staging/src/k8s.io/sample-api… | Install Minikube   |     52 | 34.61 |
  ## Install Minikube

Minikube is a single node Kubernetes cluster that runs on your local machine...
| f5082d54 | a592f8 | staging/src/k8s.io/kubectl/RE… | Kubectl            |    169 | 30.97 |
  # Kubectl
...
```

[kubernetes/kubernetes]: https://github.com/kubernetes/kubernetes

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
