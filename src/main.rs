use clap::{Parser, Subcommand};

mod cache;
mod db;
mod id;
mod ignore_cmd;
mod index;
mod outline;
mod parse;
mod repo;
mod search;
mod show;
mod sync;
mod toc;
mod token;

#[derive(Parser)]
#[command(
    name = "mdpeek",
    version,
    about = "Progressive-disclosure CLI for markdown/text docs",
    after_long_help = "NOTE: If you are an LLM or agent, run `mdpeek --llm-help` for a complete reference."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    /// Emit JSON instead of plain text
    #[arg(long, global = true)]
    json: bool,
    /// Skip the pre-flight freshness scan
    #[arg(long, global = true)]
    no_auto_index: bool,
    /// Run as if started in <PATH> instead of cwd
    #[arg(short = 'C', global = true, value_name = "PATH")]
    directory: Option<std::path::PathBuf>,
    /// Print a single comprehensive reference for LLM consumption, then exit
    #[arg(long)]
    llm_help: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Layer 1: list docs with IDs, titles, token counts (always reindexes)
    Index {
        /// Path inside the repo (defaults to cwd)
        path: Option<std::path::PathBuf>,
    },
    /// Layer 2: headings + first-sentence snippets for one or more docs
    Outline { doc_ids: Vec<String> },
    /// Cross-doc table of contents: headings for every indexed doc in one call
    Toc,
    /// Layer 3: full doc or a single section (doc-id[:section-id] or doc-id section-id)
    Show {
        /// One or more targets: doc-id, path, doc-id:section-id
        targets: Vec<String>,
        /// Truncate output to at most N tokens (token-boundary aligned)
        #[arg(long)]
        max_tokens: Option<usize>,
        /// Skip the first N tokens before emitting (use with --max-tokens for paging)
        #[arg(long, default_value_t = 0)]
        start_token: usize,
        /// Replace fenced code blocks with `[code: N lines]` placeholders
        #[arg(long)]
        no_code: bool,
    },
    /// Full-text search over the indexed corpus
    Search {
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Include the section snippet text in each result
        #[arg(long)]
        snippet: bool,
        /// Include full section content in each JSON result
        #[arg(long)]
        include_content: bool,
        /// Truncate --include-content to at most N tokens per section
        #[arg(long)]
        max_tokens: Option<usize>,
    },
    /// Manage per-repo user-ignore patterns (gitignore syntax)
    Ignore {
        /// Patterns to append
        patterns: Vec<String>,
        /// List current patterns (default action when no other flags set)
        #[arg(long)]
        list: bool,
        /// Remove a single pattern
        #[arg(long, value_name = "PATTERN")]
        remove: Option<String>,
        /// Clear all patterns
        #[arg(long)]
        clear: bool,
    },
}

const LLM_HELP: &str = "\
mdpeek — progressive-disclosure CLI for markdown/text docs

WHEN TO USE
  Use mdpeek when you need to explore documentation in a monorepo without
  reading every file. mdpeek is designed for LLMs and scripts: every
  operation is a single non-interactive command with structured I/O.

  Use mdpeek when you want to:
  - Discover what docs exist and their retrieval cost (token counts)
  - Drill into a doc's structure (headings, snippets) before reading it
  - Read a specific section by stable content-hash ID
  - Full-text search across all indexed docs
  - Manage per-repo ignore patterns for docs you don't care about

  Three progressive layers:
  1. Index  — list docs with IDs, titles, token counts (cheap scan)
  2. Outline — headings + first-sentence snippets per section
  3. Show   — full doc or a single section addressed by content hash

DO NOT USE
  - For exploring source code — mdpeek only indexes .md, .mdx, and .txt
  - For reading a single small file you already know the path to; read it
    directly instead
  - For content in files excluded by .gitignore or the repo's ignore list

DECISION TREE
  \"What docs exist here?\"                → mdpeek index
  \"What's the structure across the repo?\" → mdpeek toc
  \"What's in <doc>?\"                     → mdpeek outline <doc>
  \"Which doc mentions X?\"                → mdpeek search \"X\"
  \"Read section Y of <doc>\"              → mdpeek show <doc>:<sec>
  \"Read content about X within a budget\" → mdpeek search \"X\" \\
                                            --include-content --max-tokens 400

COMMANDS
  mdpeek index [path]
    Layer 1: force-reindex and list all docs grouped by directory.
    Shows doc ID, title, token count, and last-modified date.
      mdpeek index                       # index from cwd
      mdpeek index docs/                 # index from a subdirectory
      mdpeek --json index                # structured output

  mdpeek outline <doc-id-or-path>...
    Layer 2: show headings + first-sentence snippets for one or more docs.
    Each section shows its section ID and token count.
    Accepts doc IDs (prefix match) or repo-relative file paths.
    Multiple docs are concatenated (text) or returned as an array (JSON).
      mdpeek outline a3f1b208            # by full doc ID
      mdpeek outline a3f1                # prefix match (if unambiguous)
      mdpeek outline docs/install.md     # by file path
      mdpeek outline a3f1 b208           # multiple docs at once
      mdpeek --json outline a3f1b208     # structured output

  mdpeek toc
    Cross-doc table of contents: headings (no snippets) for every
    indexed doc in a single call. Use this instead of running outline
    over each doc when you want a whole-repo structural picture.
    Each section shows its ID, heading, and token count.
      mdpeek toc                         # plain text, all docs
      mdpeek --json toc                  # structured output

  mdpeek show <target>...
    Layer 3: print full doc content, or a single section slice.
    Accepts one or more targets. Each target is a doc ID (prefix match)
    or repo-relative file path, optionally with :section-id suffix.
    Multiple targets are concatenated (text) or returned as an array (JSON).
    --max-tokens N truncates output at a token boundary.
    --start-token N skips the first N tokens (use with --max-tokens for paging).
    --no-code replaces fenced code blocks with `[code: N lines]` placeholders
    (useful when outline shows a section has a high code_tokens ratio but you
    only need the prose).
    JSON output includes start_token, end_token, truncated fields when
    --max-tokens is set.
      mdpeek show a3f1b208               # full doc
      mdpeek show docs/install.md        # full doc by path
      mdpeek show a3f1b208:4f21a9        # one section (colon syntax)
      mdpeek show a3f1b208 sec:4f21a9    # same (two-arg, pasted from outline)
      mdpeek show a3f1:4f21 b208:9c3e    # multiple targets
      mdpeek --json show a3f1b208        # structured output with token count
      mdpeek --json show a3f1 --max-tokens 500           # first 500 tokens
      mdpeek --json show a3f1 --max-tokens 500 --start-token 500  # next page
      mdpeek show a3f1 --no-code         # strip code fences

  mdpeek search <query> [--limit N] [--snippet] [--include-content] [--max-tokens N]
    Full-text search over the indexed corpus. English stemming,
    stop-word removal, heading boost (3x). Default limit: 10.
    --snippet includes a query-relevant text excerpt in each result.
    --include-content embeds the full section content in each JSON
    result, eliminating the need for follow-up show calls.
    --max-tokens N truncates each --include-content section at N tokens
    and adds a truncated field to the JSON.
      mdpeek search \"cargo install\"      # plain table
      mdpeek --json search \"install\"     # structured output
      mdpeek search \"auth\" --limit 5     # cap results
      mdpeek --json search \"auth\" --snippet  # include snippets
      mdpeek --json search \"auth\" --include-content  # full sections
      mdpeek --json search \"auth\" --include-content --max-tokens 200  # budgeted

  mdpeek ignore <pattern>...
    Manage per-repo user-ignore patterns (gitignore syntax).
    Patterns apply in addition to .gitignore.
      mdpeek ignore 'drafts/**'          # add patterns
      mdpeek ignore --list               # show current patterns
      mdpeek ignore --remove 'drafts/**' # remove one
      mdpeek ignore --clear              # clear all

GLOBAL FLAGS
  -C <PATH>          Run as if started in <PATH> instead of cwd
  --json             Emit JSON instead of plain text
  --no-auto-index    Skip the pre-flight freshness scan (use stale cache)
  --llm-help         Print this reference and exit

IDS
  Doc ID:     8-hex SHA-256 of repo-relative path. Stable across edits.
  Section ID: 6-hex SHA-256 of section content. Stable across moves.
  Both support unambiguous prefix matching (like squire hunk IDs).

AUTO-INDEXING
  Every command (except `index`) runs a freshness check before output:
  if the last scan was <5s ago, it's skipped. `index` always forces a
  full rescan. Use --no-auto-index to skip entirely.

TYPICAL WORKFLOW
  1. mdpeek --json index                 # discover docs + token costs
  2. mdpeek --json outline <doc-id>      # drill into structure
  3. mdpeek show <doc-id>:<section-id>   # read just what you need
  4. mdpeek --json search \"query\"        # find across all docs

JSON OUTPUT
  index:   {repo_root, total_docs, total_tokens, groups:[{dir, docs:[{id, path, title, tokens, section_count, modified}]}]}
  outline: {doc_id, path, total_tokens, sections:[{id, level, heading, snippet, tokens, code_tokens}]}
  toc:     [{doc_id, path, total_tokens, sections:[{id, level, heading, heading_path, tokens}]}]
  show:    {doc_id, path, section_id?, heading?, tokens, content,
           start_token?, end_token?, truncated?}  (budget fields when --max-tokens set)
  search:  [{doc_id, section_id, path, heading_path, tokens, score, snippet?,
            content?, truncated?}]  (truncated when --max-tokens set with --include-content)
  ignore:  {patterns:[], message:\"\"}
  errors:  {\"error\": \"message\"} with non-zero exit

CACHE
  Per-repo state at $XDG_CACHE_HOME/mdpeek/repos/<repo-hash>/
  Contains mdpeek.db (sqlite) and tantivy/ (full-text index).
";

fn main() {
    let cli = Cli::parse();
    if cli.llm_help {
        print!("{LLM_HELP}");
        return;
    }
    if let Some(dir) = &cli.directory
        && let Err(e) = std::env::set_current_dir(dir)
    {
        eprintln!("error: -C {}: {e}", dir.display());
        std::process::exit(1);
    }
    let Some(cmd) = cli.cmd else {
        eprintln!("error: a subcommand is required (use --help or --llm-help)");
        std::process::exit(1);
    };
    if let Err(e) = run(cmd, cli.json, cli.no_auto_index) {
        if cli.json {
            println!("{}", serde_json::json!({"error": format!("{e:#}")}));
        } else {
            eprintln!("error: {e:#}");
        }
        std::process::exit(1);
    }
}

fn run(cmd: Cmd, json: bool, no_auto_index: bool) -> anyhow::Result<()> {
    match cmd {
        Cmd::Index { path } => index::run(path, json, no_auto_index),
        Cmd::Outline { doc_ids } => outline::run(&doc_ids, json, no_auto_index),
        Cmd::Toc => toc::run(json, no_auto_index),
        Cmd::Show {
            targets,
            max_tokens,
            start_token,
            no_code,
        } => show::run(
            &targets,
            json,
            no_auto_index,
            max_tokens,
            start_token,
            no_code,
        ),
        Cmd::Search {
            query,
            limit,
            snippet,
            include_content,
            max_tokens,
        } => search::run(
            &query,
            limit,
            snippet,
            include_content,
            max_tokens,
            json,
            no_auto_index,
        ),
        Cmd::Ignore {
            patterns,
            list,
            remove,
            clear,
        } => {
            let cwd = std::env::current_dir()?;
            ignore_cmd::run(
                ignore_cmd::IgnoreArgs {
                    add: patterns,
                    list,
                    remove,
                    clear,
                },
                &cwd,
                json,
            )
        }
    }
}
