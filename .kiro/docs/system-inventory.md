# System Inventory: Roles, `%functions%`, Agents, and Tools

This document maps out all active **Roles**, the special **`%functions%`** role, the registered **Agents**, and the atomic **Tools** in the `aichat` and `llm-functions` ecosystem.

---

## 1. System Topology & Relationship Diagram

```mermaid
graph TD
    subgraph ROLES [Roles: System Personas]
        R_Builtin["%shell%, %code%, %explain-shell%"]
        R_Custom["Custom Roles (e.g. coding-assist)"]
        R_Func["%functions% (Universal Tool Role)"]
    end

    subgraph AGENTS [Agents: Isolated Reasoning Units]
        A_Orch["orchestrator (Coordinator)"]
        A_Res["researcher (Cognitive Specialist)"]
        A_Coder["coder (Cognitive Specialist)"]
        A_Todo["todo (Deterministic SH Agent)"]
        A_Sql["sql (Deterministic SH Agent)"]
        A_Demo["demo / json-viewer"]
    end

    subgraph TOOLS [Tools: Atomic Actuators]
        T_FS["fs_cat, fs_ls, fs_mkdir, fs_patch, fs_rm, fs_write"]
        T_Web["web_search, fetch_url_via_curl, read_pdf, summarize_text"]
        T_Exec["execute_command, execute_py_code, execute_js_code, execute_sql_code"]
        T_Utils["slow_task, generate_data, get_current_weather, etc."]
    end

    R_Func -.->|Exposes all 31 tools globally| TOOLS
    
    A_Orch ==>|Delegates as Subprocess (Route 2)| A_Res
    A_Orch ==>|Delegates as Subprocess (Route 2)| A_Coder
    
    A_Res -->|Private Toolset| T_Web
    A_Coder -->|Private Toolset| T_FS
    
    A_Todo -->|Executes subcommands (Route 3)| A_Todo
    A_Sql -->|Executes SQL (Route 3)| T_Exec
```

---

## 2. Category 1: Roles (7 Total)

Roles configure prompt personas and behavioral constraints for sessions.

### Built-in System Roles
1. **`%functions%`**: Virtual role that injects all 31 global tools from `functions.json` into the model's context.
2. **`%shell%`**: Instructs the model to output a single, raw, executable shell command for the user's OS with zero explanations and no markdown fences.
3. **`%code%`**: Instructs the model to output pure code without commentary, markdown code blocks, or explanations.
4. **`%explain-shell%`**: Instructs the model to provide a clear, step-by-step explanation of a specified shell command.
5. **`%create-prompt%`**: Helper role that generates structured system prompts and role templates based on user requirements.
6. **`%create-title%`**: Helper role that generates a concise 3-5 word title for a conversation.

### Custom User Roles (`~/.config/aichat/roles/`)
7. **`coding-assist`** (`coding-assist.md`): Custom programming persona with user-tailored instructions.

---

## 3. Category 2: Agents (7 Total)

Agents are dedicated execution units with specific instructions, private toolsets, and turn loops.

| # | Agent Name | Location | Type | Description & Capabilities |
|---|:---|:---|:---|:---|
| 1 | **`orchestrator`** | `agents/orchestrator/` | Cognitive Coordinator | Decomposes complex tasks with `_plan`, delegates to `researcher` and `coder` via subprocesses (**Route 2**), and synthesizes results. |
| 2 | **`researcher`** | `agents/researcher/` | Cognitive Specialist | Information gathering specialist. Runs an autonomous multi-turn loop (**Route 2**) with `web_search.sh` and `fetch_url_via_curl.sh` (piped to `summarize_text.sh`). |
| 3 | **`coder`** | `agents/coder/` | Cognitive Specialist | Coding and workspace management specialist. Private toolset scoped to `fs_cat`, `fs_ls`, `fs_mkdir`, `fs_patch`, `fs_rm`, `fs_write`. |
| 4 | **`todo`** | `agents/todo/` | Deterministic SH Agent | Local task manager. Exposes subcommands `add_todo`, `list_todos`, `complete_todo`, `delete_todo`, `clean_todos` via bash shim `bin/todo` (**Route 3**). |
| 5 | **`sql`** | `agents/sql/` | Deterministic SH Agent | SQL query runner and database introspection shim via `bin/sql` (**Route 3**). |
| 6 | **`demo`** | `agents/demo/` | Deterministic Agent | Demonstration and testing multi-tool shim via `bin/demo` (**Route 3**). |
| 7 | **`json-viewer`**| `agents/json-viewer/` | Deterministic Agent | Structured JSON inspection and formatting shim via `bin/json-viewer` (**Route 3**). |

---

## 4. Category 3: The `%functions%` Suite / Tools (31 Total)

All 31 atomic tools located in `llm-functions/tools/` and compiled into `functions.json`:

### A. Filesystem Tools (6 Tools)
1. **`fs_cat.sh`**: Read the contents of a file at a specified path (with auto-capping protection).
2. **`fs_ls.sh`**: List files and directories at a given path.
3. **`fs_mkdir.sh`**: Create a new directory and any necessary parent directories.
4. **`fs_patch.sh`**: Apply a unified diff patch to a file.
5. **`fs_rm.sh`**: Delete a file or recursively delete a directory.
6. **`fs_write.sh`**: Write content directly to a file (creating or overwriting).

### B. Web Search & Intelligence Tools (7 Tools)
7. **`web_search.sh`**: Unified symlink interface pointing to the active search provider backend (`web_search_aichat.sh`).
8. **`web_search_aichat.sh`**: Google Gemini / VertexAI Grounded Search backend (`gemini-3.5-flash`).
9. **`web_search_perplexity.sh`**: Search backend powered by the Perplexity API.
10. **`web_search_tavily.sh`**: Search backend powered by the Tavily AI search API.
11. **`search_wikipedia.sh`**: Query and retrieve extracts from Wikipedia articles.
12. **`search_arxiv.sh`**: Search academic preprints and research papers on arXiv.
13. **`search_wolframalpha.sh`**: Query WolframAlpha for computational knowledge and exact answers.

### C. Web Ingestion & Text Processing (5 Tools)
14. **`fetch_url_via_curl.sh`**: Fetch webpage content using curl and convert HTML to markdown (configured with pipe routing to `summarize_text`).
15. **`fetch_url_via_jina.sh`**: Fetch and parse webpage content using the Jina Reader API.
16. **`fetch_and_summarize.sh`**: Integrated pipeline that fetches a URL and produces a condensed summary.
17. **`summarize_text.sh`**: LLM-powered summarizer (`gemini-3.5-flash`) that digests large texts while passing short inputs (<1KB) through raw.
18. **`read_pdf.sh`**: Extract text, select page ranges (`--pages 5-10`), and format PDFs using `pdftotext`.

### D. Code & Command Execution (5 Tools)
19. **`execute_command.sh`**: Run arbitrary bash shell commands on the local system.
20. **`execute_py_code.py`**: Execute Python scripts in an isolated execution environment.
21. **`execute_js_code.js`**: Execute JavaScript/Node.js scripts.
22. **`execute_sql_code.sh`**: Execute SQL statements against local SQLite or configured databases.
23. **`demo_sh.sh`** / **`demo_js.js`** / **`demo_py.py`**: Multi-language test harnesses demonstrating argc shim integrations.

### E. Utilities, Generators & Notifications (8 Tools)
24. **`generate_data.sh`**: Generates mock CSV/JSON datasets (configured with file destination routing to `/tmp/generate_data-*.csv`).
25. **`slow_task.sh`**: Artificial delay tool (used to test parallel scheduling and observability).
26. **`get_current_time.sh`**: Retrieve the current local timestamp, timezone, and date.
27. **`get_current_weather.sh`**: Fetch real-time meteorological conditions for a given location.
28. **`send_mail.sh`**: Dispatch emails via SMTP or configured mail transfer agents.
29. **`send_twilio.sh`**: Dispatch SMS text messages using the Twilio API.
30. **`demo_js.js`**: JavaScript integration demo tool.
31. **`demo_py.py`**: Python integration demo tool.

---

## 5. Category 4: In-Process Pseudo-Tools (1 Total)

* **`_plan`**: Built-in Rust pseudo-tool injected into the agent loop. Provides an internal scratchpad for task decomposition without generating external process or user-facing output.

---

## 6. Category 5: Registered RAG Knowledge Bases

The system has access to pre-indexed local RAG knowledge stores located in `~/.config/aichat/rags/`:

### 1. **`aichat-upstream-wiki`**
* **Source Documents:** `https://github.com/sigoden/aichat/wiki/**`
* **Config File:** `~/.config/aichat/rags/aichat-upstream-wiki.yaml`
* **Embedding Model:** `gemini:gemini-embedding-001` (Chunk size: 1536, Overlap: 76, Top-K: 5)
* **How to Query:**
  ```bash
  /usr/bin/aichat -S --rag aichat-upstream-wiki "<question>"
  ```
* **Indexed Coverage:** Complete official documentation and reference guides for upstream `sigoden/aichat`, including:
  * **Chat-REPL Guide** (Keyboard shortcuts, commands, multi-line input)
  * **Command-Line Guide** (All CLI flags, pipe integration, output modes)
  * **Role Guide** (System prompts, variables, `%shell%`, `%code%`, `%functions%`)
  * **Macro Guide** (Command macro definitions and chaining)
  * **RAG Guide** (Embedding models, vector stores, chunking parameters)
  * **Environment Variables** (Global configuration, logging levels, custom paths)
  * **Configuration Guide** (Model providers, API endpoints, YAML options)
  * **Custom Themes & REPL Prompts** (ANSI coloring, status states)
  * **FAQ & Troubleshooting**
* **Researcher Note:** Any autonomous agent, researcher, or developer investigating upstream `aichat` behavior, built-in features, or configuration specifications should consult this RAG as the primary source of truth.
