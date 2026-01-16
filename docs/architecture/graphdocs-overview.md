# GraphDocs Architecture Overview

GraphDocs is **not** about storing raw markdown files in a graph. It's about **decomposing documents into a graph structure** and **rendering them on-demand**.

## The Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│                         IMPORT PHASE                                │
│                                                                     │
│   README.md ──────► Parser ──────► Graph Tables (DuckDB)           │
│   (source file)     (2.1)          (gd_documents, gd_sections,     │
│                                     gd_variables, gd_edges)         │
└─────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      STORAGE (Graph Structure)                      │
│                                                                     │
│   Document: "readme"                                                │
│       │                                                             │
│       ├── Section: "# {{project_name}}"  (heading, level 1)        │
│       │       │                                                     │
│       │       └──[follows]──► Section: "{{description}}"           │
│       │                              │                              │
│       │                              └──[follows]──► Section: ...   │
│       │                                                             │
│       └── Variables: project_name="AgentFS", description="..."     │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────────┐
│                       RENDER PHASE (FUSE Mount)                     │
│                                                                     │
│   cat /mnt/agent/.graphdocs/readme.gd.md                           │
│                     │                                               │
│                     ▼                                               │
│   GraphDocsEngine: Load sections → Substitute variables → Markdown │
│                     │                                               │
│                     ▼                                               │
│   Output: "# AgentFS\n\nA filesystem for AI agents\n\n..."         │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Key Concepts

| Concept | Description |
|---------|-------------|
| **Decomposition** | Markdown is parsed into discrete sections (headings, paragraphs, code blocks) |
| **Graph Storage** | Sections stored in `gd_sections`, relationships in `gd_edges` |
| **Variables** | `{{placeholders}}` are stored separately, allowing dynamic substitution |
| **Inheritance** | Documents can inherit from templates (`base_template`) |
| **On-Demand Render** | When you `cat file.gd.md`, the engine assembles sections + substitutes variables |
| **Time-Travel** | Because it's in DuckDB with journaling, you can render past versions |

## Why This Architecture?

1. **Reusable Templates** - Create a README template, inherit it across 50 projects, change variables only
2. **Programmatic Editing** - Modify a single section without parsing the whole file
3. **Graph Queries** - "Find all sections that reference the API" via DuckPGQ
4. **Versioning** - Track section-level changes, not just file-level
5. **AI-Friendly** - Agents can manipulate structured sections, not raw text

## Example Workflow

```bash
# 1. Import existing markdown into graph
agentfs graphdocs import README.md --id readme

# 2. Set variables
agentfs graphdocs set-var readme project_name "AgentFS"
agentfs graphdocs set-var readme version "2.0.0"

# 3. Mount filesystem
agentfs mount my-agent /mnt/agent --enable-graphdocs

# 4. Read rendered document (variables substituted)
cat /mnt/agent/.graphdocs/readme.gd.md

# 5. The actual files in the regular FS are unchanged
# GraphDocs lives in the DuckDB database, rendered on access
```

## Layers

- **Consumption Layer**: FUSE filesystem mount (read `.gd.md` files)
- **Storage Layer**: DuckDB graph tables (`gd_documents`, `gd_sections`, `gd_variables`, `gd_edges`)
- **Management Layer**: CLI commands (`agentfs graphdocs import/render/set-var/etc.`)

## Related Stories

| Story | Description | Status |
|-------|-------------|--------|
| STORY-1.1 | Base Tables (schema) | Done |
| STORY-1.2 | Property Graph Definition | Done |
| STORY-2.1 | Markdown Parser | Ready |
| STORY-2.2 | LLM Schema Converter | Ready |
| STORY-2.3 | Import CLI | Ready |
| STORY-3.1 | GraphDocsEngine | Ready |
| STORY-3.2 | Template Inheritance | Ready |
| STORY-3.3 | Time-Travel Rendering | Ready |
| STORY-4.1 | GraphDocsHandler (FUSE) | Done |
| STORY-4.2 | Virtual Directory Listing | Ready |
| STORY-5.1 | CLI GraphDocs | Ready |
| STORY-5.2 | Editor Integration | Ready |
