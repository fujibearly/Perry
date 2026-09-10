# AIChat: All-in-one LLM CLI Tool

[![CI](https://github.com/sigoden/aichat/actions/workflows/ci.yaml/badge.svg)](https://github.com/sigoden/aichat/actions/workflows/ci.yaml)
[![Crates](https://img.shields.io/crates/v/aichat.svg)](https://crates.io/crates/aichat)
[![Discord](https://img.shields.io/discord/1226737085453701222?label=Discord)](https://discord.gg/mr3ZZUB9hG)

AIChat is an all-in-one LLM CLI tool featuring Shell Assistant, CMD & REPL Mode, RAG, AI Tools & Agents, and More. 

## Install

### Package Managers

- **Rust Developers:** `cargo install aichat`
- **Homebrew/Linuxbrew Users:** `brew install aichat`
- **[mise](https://mise.jdx.dev/) Users:** `mise use --global aqua:sigoden/aichat@latest`
- **Pacman Users**: `pacman -S aichat`
- **Windows Scoop Users:** `scoop install aichat`
- **Windows WinGet Users:** `winget install --exact --id sigoden.AIChat`
- **Android Termux Users:** `pkg install aichat`

### Pre-built Binaries

Download pre-built binaries for macOS, Linux, and Windows from [GitHub Releases](https://github.com/sigoden/aichat/releases), extract them, and add the `aichat` binary to your `$PATH`.

## Features

### Multi-Providers

Integrate seamlessly with over 20 leading LLM providers through a unified interface. Supported providers include OpenAI, Claude, Gemini (Google AI Studio), Ollama, Groq, Azure-OpenAI, VertexAI, Bedrock, Github Models, Mistral, Deepseek, AI21, XAI Grok, Cohere, Perplexity, Cloudflare, OpenRouter, Ernie, Qianwen, Moonshot, ZhipuAI, MiniMax, Deepinfra, VoyageAI, any OpenAI-Compatible API provider.

### CMD Mode

Explore powerful command-line functionalities with AIChat's CMD mode.

![aichat-cmd](https://github.com/user-attachments/assets/6c58c549-1564-43cf-b772-e1c9fe91d19c)

### REPL Mode

Experience an interactive Chat-REPL with features like tab autocompletion, multi-line input support, history search, configurable keybindings, and custom REPL prompts.

Editor commands from the `editor` configuration, `VISUAL`, or `EDITOR` may include arguments. They use POSIX shell-word quoting on every platform, so executable paths containing spaces must be quoted, for example `"C:\Program Files\Helix\hx.exe" --wait`.

Use `--no-spinner` to suppress animated progress indicators, which is useful when piping output or running in non-interactive environments.

![aichat-repl](https://github.com/user-attachments/assets/218fab08-cdae-4c3b-bcf8-39b6651f1362)

### Shell Assistant

Elevate your command-line efficiency. Describe your tasks in natural language, and let AIChat transform them into precise shell commands. AIChat intelligently adjusts to your OS and shell environment.

![aichat-execute](https://github.com/user-attachments/assets/0c77e901-0da2-4151-aefc-a2af96bbb004)

### Multi-Form Input

Accept diverse input forms such as stdin, local files and directories, and remote URLs, allowing flexibility in data handling.

| Input             | CMD                                  | REPL                             |
| ----------------- | ------------------------------------ | -------------------------------- |
| CMD               | `aichat hello`                       |                                  |
| STDIN             | `cat data.txt \| aichat`             |                                  |
| Last Reply        |                                      | `.file %%`                       |
| Local files       | `aichat -f image.png -f data.txt`    | `.file image.png data.txt`       |
| Shell-expanded files | `aichat --files src/*.rs -- explain` | `.file src/a.rs src/b.rs -- explain` |
| Local directories | `aichat -f dir/`                     | `.file dir/`                     |
| Remote URLs       | `aichat -f https://example.com`      | `.file https://example.com`      |
| External commands | ```aichat -f '`git diff`'```         | ```.file `git diff` ```          |
| Combine Inputs    | `aichat -f dir/ -f data.txt explain` | `.file dir/ data.txt -- explain` |

The `--files` flag accepts shell-expanded paths (your shell expands globs before AIChat sees them). Use `--` to separate file arguments from the prompt text. This is equivalent to multiple `-f` flags but more convenient for wildcard patterns.

### Role

Customize roles to tailor LLM behavior, enhancing interaction efficiency and boosting productivity.

![aichat-role](https://github.com/user-attachments/assets/023df6d2-409c-40bd-ac93-4174fd72f030)

> The role consists of a prompt and model configuration.

### Session

Maintain context-aware conversations through sessions, ensuring continuity in interactions.

![aichat-session](https://github.com/user-attachments/assets/56583566-0f43-435f-95b3-730ae55df031)

> The left side uses a session, while the right side does not use a session.

### Macro

Streamline repetitive tasks by combining a series of REPL commands into a custom macro.

![aichat-macro](https://github.com/user-attachments/assets/23c2a08f-5bd7-4bf3-817c-c484aa74a651)

### RAG

Integrate external documents into your LLM conversations for more accurate and contextually relevant responses.

![aichat-rag](https://github.com/user-attachments/assets/359f0cb8-ee37-432f-a89f-96a2ebab01f6)

### Function Calling

Function calling supercharges LLMs by connecting them to external tools and data sources. This unlocks a world of possibilities, enabling LLMs to go beyond their core capabilities and tackle a wider range of tasks.

We have created a new repository [https://github.com/sigoden/llm-functions](https://github.com/sigoden/llm-functions) to help you make the most of this feature.

#### AI Tools & MCP

Integrate external tools to automate tasks, retrieve information, and perform actions directly within your workflow.

![aichat-tool](https://github.com/user-attachments/assets/7459a111-7258-4ef0-a2dd-624d0f1b4f92)

#### AI Agents (CLI version of OpenAI GPTs)

AI Agent = Instructions (Prompt) + Tools (Function Callings) + Documents (RAG).

![aichat-agent](https://github.com/user-attachments/assets/0b7e687d-e642-4e8a-b1c1-d2d9b2da2b6b)

A minimal local agent requires an agent definition and an entry in `agents.txt` under the functions directory. The agent-specific `config.yaml` is optional and only overrides runtime configuration such as the model, temperature, or default variables.

```text
<aichat-config-dir>/
  functions/
    agents.txt                  # contains: my-agent
    agents/
      my-agent/
        index.yaml              # required agent definition
        functions.json          # optional tools
  agents/
    my-agent/
      config.yaml               # optional agent-specific config
```

Example `functions/agents.txt`:

```text
my-agent
```

Example `functions/agents/my-agent/index.yaml`:

```yaml
name: my-agent
description: Helps with local project tasks.
instructions: |
  You are a concise assistant for this project.
```

After creating these files, run `aichat --list-agents` to confirm that the agent is discoverable, then use it with `aichat -a my-agent`.

### Local Server Capabilities

AIChat includes a lightweight built-in HTTP server for easy deployment.

```
$ aichat --serve
Chat Completions API: http://127.0.0.1:8000/v1/chat/completions
Embeddings API:       http://127.0.0.1:8000/v1/embeddings
Rerank API:           http://127.0.0.1:8000/v1/rerank
LLM Playground:       http://127.0.0.1:8000/playground
LLM Arena:            http://127.0.0.1:8000/arena?num=2
```

#### Proxy LLM APIs

The LLM Arena is a web-based platform where you can compare different LLMs side-by-side. 

Test with curl:

```sh
curl -X POST -H "Content-Type: application/json" -d '{
  "model":"claude:claude-3-5-sonnet-20240620",
  "messages":[{"role":"user","content":"hello"}], 
  "stream":true
}' http://127.0.0.1:8000/v1/chat/completions
```

#### LLM Playground

A web application to interact with supported LLMs directly from your browser.

![aichat-llm-playground](https://github.com/user-attachments/assets/aab1e124-1274-4452-b703-ef15cda55439)

#### LLM Arena

A web platform to compare different LLMs side-by-side.

![aichat-llm-arena](https://github.com/user-attachments/assets/edabba53-a1ef-4817-9153-38542ffbfec6)

## Fork Enhancements

This fork preserves upstream's philosophy — tools are shell scripts, roles are markdown prompts, agents compose both — but adds **runtime intelligence to the dispatch layer**. The same definitions run through a fundamentally better engine without any format changes.

### Native MCP Bridge

Replaced the Node.js MCP bridge with an in-process Rust implementation. MCP servers are spawned as child processes, communicate via JSON-RPC 2.0 over stdio, and their tools appear identically to shell-exec tools — no new abstractions.

```yaml
mcp_servers:
  - name: filesystem
    command: mcp-server-filesystem
    args: ["/home/user/projects"]
  - name: git
    command: mcp-server-git
    args: [--repository, .]
```

- Cached manifests for fast startup (`--sync-mcp` to refresh)
- Behind `mcp` cargo feature flag (default on)
- Agent-level MCP servers supported

### Provider-Agnostic Agent Loop

An iterative agent loop that makes **every provider** capable of multi-step agentic work — not just OpenAI. Works with Claude, Gemini (via OpenRouter), Cohere, DeepSeek, local models via Ollama, or any provider that returns `tool_calls`.

- **Parallel tool execution** — multiple tool calls run concurrently (semaphore-bounded, default 8). A turn with 5 web fetches takes 1x latency, not 5x.
- **Turn budget** — configurable `max_turns` (default 20) prevents runaway. The model can't loop forever.
- **Empty-turn resilience & retry** — transient provider dropouts yielding 0 text and 0 tool calls trigger automatic backoff retry (`MAX_EMPTY_RETRIES = 2`), preventing premature loop exits.
- **Planning tool** — built-in `_plan` pseudo-tool auto-injected when tools are configured. The model can reason and decompose tasks without polluting user-visible output.
- **Sub-agent delegation** — tools marked `agent: true` spawn a new aichat process as a subprocess. Each sub-agent has its own PID, turn budget, session, and observability. Sub-agents can themselves delegate to further sub-agents (bounded by `max_agent_depth`).
- **Recursive orchestration** — an orchestrator agent can delegate to a researcher agent, which can delegate to a deep-researcher agent. Each is a full aichat instance. Depth tracked via `AICHAT_AGENT_DEPTH` env var.

```yaml
agent_loop:
  max_turns: 20
  max_concurrency: 8
  max_agent_depth: 3
  show_trace: false
  planning_tool: true
```

Environment overrides: `AICHAT_AGENT_LOOP_MAX_TURNS`, `AICHAT_AGENT_LOOP_SHOW_TRACE`

#### Example: multi-agent orchestration

```yaml
# agents/project-manager/functions.json
[
  {"name": "researcher", "description": "Research a topic", "parameters": {"type": "object", "properties": {"task": {"type": "string"}}}, "agent": true},
  {"name": "implementer", "description": "Implement a solution", "parameters": {"type": "object", "properties": {"task": {"type": "string"}}}, "agent": true}
]
```

```bash
AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat --agent project-manager "build a report on Rust async patterns"
```

The project-manager's prompt tells it to delegate. Each sub-agent runs independently with its own tools and budget.

### External Observability (designed for tmux)

The agent loop emits signals for external management tools (tmux, Herdr, Agent Deck) to observe aichat without parsing stdout:

- **OSC terminal title** — live state in tmux pane title (`aichat: turn 3/20 | fs_write`)
- **JSON status file** — `$XDG_RUNTIME_DIR/aichat-<pid>.json` for dashboards/pollers
- **BEL + OSC 777** — desktop notifications on task completion (tmux `monitor-bell`, Ghostty/iTerm2 native notifications)

Each sub-agent process writes its own independent status file. External tools enumerate `aichat-*.json` files for a fleet view — no coordination needed between processes.

```bash
# See all running aichat agents
cat /run/user/1000/aichat-*.json | jq '{pid, state, turn, max_turns, active_tools}'
```

### Tool Output Routing

Control where tool results go instead of always stuffing them into the LLM's conversation context. Declared per-tool in `functions.json`:

- **context** (default) — result goes into the next LLM turn. Auto-capped at `tool_output_limit` (default 16 KB): large results are written to a temp file and the model receives a preview + path.
- **file** — result written to a path (with template expansion), model gets a confirmation (`{"written_to": "/tmp/report.md", "size_bytes": 24576}`).
- **pipe** — result passed directly to another tool without an LLM round-trip. The model sees only the final output.

```json
[
  {
    "name": "generate_report",
    "description": "Generate a markdown report",
    "parameters": {"type": "object", "properties": {"topic": {"type": "string"}}},
    "output": {"destination": "file", "path": "/tmp/{{name}}-{{timestamp}}.md"}
  },
  {
    "name": "fetch_raw_data",
    "description": "Fetch raw dataset",
    "parameters": {"type": "object", "properties": {"url": {"type": "string"}}},
    "output": {"destination": "pipe", "target": "summarize_data"}
  }
]
```

Benefits:
- Prevents context window pollution from large tool outputs
- Enables tool pipelines (fetch → transform → summarize) without LLM round-trips per step
- Reduces token cost for workflows that produce artifacts
- Pipe chains are acyclic (cycle detection prevents infinite loops)

### Tool Safety Modes & Actuation Governance

A graduated, deterministic safety layer that governs *which* agent may perform *which* action — so autonomous sub-agents operating on real infrastructure can triage in parallel without risking accidental mutations. Fully deterministic (no LLM); the richer LLM-evaluator and human-escalation layers are staged increments (#6c/#6d) that degrade back to this floor.

**Tools declare a blast-radius tier** in `functions.json` via a `# @meta risk <tier>` annotation on the tool script (compiled by `argc build`), ordered `safe < reversible < disruptive < destructive < catastrophic`. A tool may also declare `# @meta reversible true` when its action is trivially undone. Both are governance metadata — never shown to the LLM.

```bash
# in an llm-functions tool script
# @describe Remove a file or directory
# @meta risk destructive
```

**Capability mask (sub-agents are read-only by default).** Every spawned sub-agent inherits `AICHAT_CAPABILITY_MASK=readonly` — it may only run `safe`/read-only tools. Only the top-level operator (or an agent granted more) actuates state-changing tools. Delegation itself is never blocked (spawning a sub-agent is orchestration; the child's own actions are what get gated).

**Authority ceiling (grows toward the root).** Each agent has a maximum tier it may actuate autonomously (`safety.default_ceiling`, default `destructive` — so `catastrophic` always requires a human). A parent may only *lower* the ceiling it grants a child. An over-ceiling action is refused with a structured `authority_exceeded` result (it never runs); once escalation (#6d) lands, that refusal becomes an escalation instead of a block.

**Protected Policy File (non-pardonable).** An optional owner-only YAML file of deterministic rules that can only *raise* an action's tier or *forbid* it — matched on tool name and/or argument content, so e.g. an `execute_command` containing `rm -rf` can be pushed to `catastrophic`:

```yaml
# safety.policy_file — rejected if group/world-readable
rules:
  - tool: "fs_*"
    arg_glob: "/etc/**"
    raise: catastrophic
  - tool: execute_command
    arg_contains: "rm -rf"
    raise: catastrophic
  - tool: "*"
    arg_contains: "prod"
    forbid: true
```

```yaml
safety:
  policy_file: ~/.config/aichat/policy.yaml
  default_ceiling: destructive   # catastrophic → human
```

Environment overrides: `AICHAT_SAFETY_POLICY_FILE`, `AICHAT_SAFETY_DEFAULT_CEILING`. Unclassified tools (including MCP tools, which carry no metadata) are conservatively human-reserved by default. Trace events follow a scannable grammar (`ALLOW <tool>: risk <tier> <= ceiling <tier>` and `BLOCK <tool>: risk <tier> > ceiling <tier>`), clearly annotating effective reversibility discounts or policy raises — the tool binary never runs on a block.

### Structured PDF Loading

Default document loader upgraded from `pdftotext` (plain text, no structure) to `pdf2md` ([firecrawl/pdf-inspector](https://github.com/firecrawl/pdf-inspector)) — structured Markdown with headings, tables, lists, code blocks, and formatting preserved.

```yaml
document_loaders:
  pdf: 'pdf2md --compact --raw $1'
```

Benefits for RAG: the chunker gets Markdown with structure, so chunks respect heading boundaries. Tables don't get split mid-row. 30-40% fewer tokens for the same information content compared to flat text extraction.

Install: `cargo install pdf-inspector`

## Advanced

### Reasoning Effort

Control how much reasoning a model applies by appending an effort level to the model name:

```sh
aichat -m openai:gpt-5.6-sol:high "Prove that sqrt(2) is irrational"
aichat -m claude:claude-opus-4-7:medium "Summarize this paper"
aichat -m bedrock:us.anthropic.claude-opus-4-7:low "Quick answer"
```

The syntax is `provider:model-name:effort`. When reasoning is active, `temperature` and `top_p` are automatically removed from the request since they conflict with structured reasoning.

Supported providers and effort levels:

| Provider | Effort Levels | Request Shape |
| -------- | ------------- | ------------- |
| OpenAI | none, low, medium, high, xhigh, max | `reasoning_effort` parameter |
| Claude | low, medium, high, xhigh, max | `thinking.type: adaptive` + `output_config.effort` |
| VertexAI (Claude) | low, medium, high, xhigh, max | Same as Claude |
| Bedrock (Claude) | low, medium, high, xhigh, max | Nested in `additionalModelRequestFields` |
| Gemini / VertexAI (Gemini) | low, medium, high | `generationConfig.thinkingConfig.thinkingLevel` |

If a model does not support the requested effort, AIChat rejects the request locally before contacting the API. Models without cataloged `reasoning_efforts` cannot use effort suffixes unless configured explicitly.

### Token Usage and Cost

Use `--show-cost` to print provider-reported token usage and the estimated USD cost after a response:

```sh
aichat --show-cost "Explain this code"
```

Set `show_cost: true` in the config file to enable it by default, including in the REPL. The summary is written to stderr so response text on stdout remains safe to pipe. Cost requires both usage data from the provider and input/output prices in the model catalog.

For OpenAI Responses multi-agent runs, the estimate is calculated separately for each continuation request. It accounts for cached input, cache writes, the actual service tier, GPT-5.6 long-context multipliers, and billable hosted web-search actions. Search actions are charged at the cataloged $0.01 per call; page opens and in-page finds are not counted as additional searches. If an exact calculation is not possible, the cost is reported as unavailable instead of assuming a pricing tier. Cost is also unavailable for custom or regional OpenAI `api_base` endpoints because their pricing may differ from the public API catalog.

The API reports response-level usage for the entire agent tree, not per-agent usage, so AIChat does not invent per-agent token or cost totals. If a later continuation request fails or the run is aborted, completed and otherwise billable response payloads are printed as partial usage before the error.

### OpenAI Responses Multi-agent

GPT-5.6 models can use OpenAI's hosted multi-agent orchestration in one-shot command mode. For a research-oriented default, add this to `config.yaml`:

```yaml
multi_agent:
  hosted_tools:
    - type: web_search
      search_context_size: high
      external_web_access: true
      return_token_budget: default
  tool_choice: required
  max_output_tokens: 16000
  service_tier: default
```

Then the hosted web-search tool is available to the root agent and every subagent:

```sh
aichat --show-cost --multi-agent -m openai:gpt-5.6-sol:high \
  "perform siem systems market analysis"
```

`--web-search` is a CLI shortcut that enables the default hosted web-search configuration for one run. `--max-output-tokens`, `--service-tier`, and `--max-concurrent-subagents` override their config values. OpenAI currently does not support `max_tool_calls` when multi-agent is enabled, so AIChat does not expose that control.

`--show-agent-trace` writes a sanitized structural trace to stderr. It shows response turns, agent paths, collaboration actions, message direction, phases, and tool names without printing encrypted messages, prompts, tool arguments, tool results, search queries, or search results. Set `multi_agent.show_trace: true` to enable it in the config. Web citations and returned sources are rendered as a deduplicated Markdown `Sources:` list.

Subagents receive both the local developer functions selected by `use_tools` and configured hosted tools. A local function named `web_search` remains distinct from the OpenAI-hosted `web_search` tool. First-class hosted tools require the canonical `https://api.openai.com/v1/responses` endpoint.

Multi-agent HTTP runs use Responses server-sent events and take the complete response from the terminal `response.completed`, `response.failed`, or `response.incomplete` event. AIChat disables automatic EventSource reconnection so a dropped stream cannot silently replay a potentially billable POST. A transient HTTP error may be retried only before the stream opens; later transport failures include the response position when available and remain non-retryable. Responses patches must preserve `stream: true`.

Advanced transports can patch Responses requests separately from Chat Completions:

```yaml
clients:
  - type: openai
    patch:
      responses:
        'gpt-5\.6-.*':
          headers:
            x-example: value
```

The equivalent environment override is `AICHAT_PATCH_OPENAI_RESPONSES`. Responses patches are applied after AIChat builds the first-class body; JSON Merge Patch replaces arrays, so a patched `tools` array replaces both hosted and developer tools. See the [OpenAI multi-agent guide](https://developers.openai.com/api/docs/guides/responses-multi-agent) and [web-search guide](https://developers.openai.com/api/docs/guides/tools-web-search) for the server-side contract.

### Error Handling & Retries

AIChat automatically retries requests that fail with transient errors:

- **Rate limits** (HTTP 429) and **server errors** (HTTP 5xx) are retried up to 2 times.
- If the provider includes a retry delay (e.g. `Retry-After`), AIChat honors it, capped at 30 seconds. Otherwise, exponential backoff is used (1s, 2s).
- **Non-transient errors** (authentication failures, bad requests, context length exceeded) fail immediately without retry.
- **Streaming:** only the initial connection is retried. Once the first event is delivered, mid-stream failures are not retried to avoid duplicating output.
- **Truncated streams** are rejected as errors. If a provider drops the connection before sending a completion signal, AIChat reports it as a failure rather than silently returning partial data.

No configuration is required. Retry behavior is automatic and logged at debug level (`AICHAT_LOG_LEVEL=debug`).

### Configuration References

Configuration values support environment variable references, so you can keep secrets out of config files:

```yaml
clients:
  - type: openai
    api_key: ${OPENAI_API_KEY}
  - type: claude
    api_key: $ANTHROPIC_API_KEY
```

Syntax:
- `$VAR` or `${VAR}` — replaced with the environment variable value
- `$$` — literal dollar sign (escape)
- Missing or empty variables produce a clear error at startup

Values are trimmed of surrounding whitespace after resolution. The Claude client also accepts `ANTHROPIC_API_KEY` as a fallback when the conventional `CLAUDE_API_KEY` is not set.

### Security

For self-hosted deployments using `--serve`:

- **Markdown sanitization:** The Playground and Arena web pages sanitize all rendered Markdown with [DOMPurify](https://github.com/cure53/DOMPurify) (pinned version, subresource integrity). The sanitizer loads before the Markdown parser and the page fails closed if it is unavailable.
- **Loader hardening:** Document loader placeholder expansion (`$1`, `$2`) is processed character-by-character to prevent shell injection. Paths beginning with `-` are prefixed with `./` to avoid being interpreted as flags.
- **Tool error containment:** When a tool call fails (unknown tool, invalid arguments, non-zero exit), the failure is returned as a structured result to the model rather than crashing the process.

## Custom Themes

AIChat supports custom dark and light themes, which highlight response text and code blocks.

![aichat-themes](https://github.com/sigoden/aichat/assets/4012553/29fa8b79-031e-405d-9caa-70d24fa0acf8)

## Documentation

- [Chat-REPL Guide](https://github.com/sigoden/aichat/wiki/Chat-REPL-Guide)
- [Command-Line Guide](https://github.com/sigoden/aichat/wiki/Command-Line-Guide)
- [Role Guide](https://github.com/sigoden/aichat/wiki/Role-Guide)
- [Macro Guide](https://github.com/sigoden/aichat/wiki/Macro-Guide)
- [RAG Guide](https://github.com/sigoden/aichat/wiki/RAG-Guide)
- [Environment Variables](https://github.com/sigoden/aichat/wiki/Environment-Variables)
- [Configuration Guide](https://github.com/sigoden/aichat/wiki/Configuration-Guide)
- [Custom Theme](https://github.com/sigoden/aichat/wiki/Custom-Theme)
- [Custom REPL Prompt](https://github.com/sigoden/aichat/wiki/Custom-REPL-Prompt)
- [FAQ](https://github.com/sigoden/aichat/wiki/FAQ)
- [Changelog](CHANGELOG.md)

## License

Copyright (c) 2023-2025 aichat-developers.

AIChat is made available under the terms of either the MIT License or the Apache License 2.0, at your option.

See the LICENSE-APACHE and LICENSE-MIT files for license details.
