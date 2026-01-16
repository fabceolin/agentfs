# EPIC-GRAPHDOCS-001: GraphDocs - Documentos Renderizados de Grafos

> **NOTA**: Esta documentacao e conceitual. Durante a fase de implementacao, alteracoes podem ser feitas para adaptar aos requisitos reais.

## Visao Geral

| Campo | Valor |
|-------|-------|
| **ID** | EPIC-GRAPHDOCS-001 |
| **Titulo** | GraphDocs - Documentos como Grafos de Propriedades |
| **Status** | Draft |
| **Prioridade** | Media |
| **Dependencias** | EPIC-DUCKAGENTFS-001 |

## Descricao

Implementar um sistema onde documentos Markdown sao armazenados como grafos de propriedades no DuckDB (usando DuckPGQ) e renderizados dinamicamente quando acessados via FUSE.

### Caso de Uso Principal

```bash
# O arquivo nao existe fisicamente, e renderizado do grafo
$ cat /agentfs/docs/project-overview.gd.md

# Output: Markdown renderizado com variaveis substituidas
# My Project
Version: 1.0.0
Status: Production

## Features
- Feature A
- Feature B
```

### Caracteristicas

1. **Heranca de Templates**: Documentos podem herdar de outros documentos
2. **Variaveis**: Substituicao de `{{variable}}` por valores do grafo
3. **Secoes Estruturadas**: Documentos compostos de secoes conectadas
4. **Time-Travel**: Versoes anteriores acessiveis via event_id
5. **Read-Only via FUSE**: Arquivos `.gd.md` sao somente leitura (editar via grafo)

## Stories

### Fase 1: Schema GraphDocs

#### STORY-1.1: Tabelas Base
**Como** desenvolvedor
**Quero** tabelas para armazenar documentos como grafos
**Para** ter a estrutura de dados necessaria

**Criterios de Aceitacao**:
- [x] Tabela `gd_documents` (metadados do documento)
- [x] Tabela `gd_sections` (secoes do documento)
- [x] Tabela `gd_variables` (variaveis para substituicao)
- [x] Tabela `gd_edges` (relacoes entre secoes)
- [x] Indices para queries eficientes

**Arquivo**: `schema/duckagentfs.sql`

---

#### STORY-1.2: Property Graph Definition
**Como** desenvolvedor
**Quero** definir o grafo de propriedades
**Para** usar DuckPGQ para queries

**Criterios de Aceitacao**:
- [x] CREATE PROPERTY GRAPH graphdocs (comentado no schema)
- [x] Vertices: documents, sections, variables
- [x] Edges: has_section, has_variable, edge

**Arquivo**: `schema/duckagentfs.sql`

---

### Fase 2: Parsing e Populacao

#### STORY-2.1: Markdown Parser (Parent Story)
**Como** desenvolvedor
**Quero** um parser de Markdown para grafos
**Para** converter documentos existentes

**Sub-Stories**:

| Sub-Story | Descricao | Status |
|-----------|-----------|--------|
| [STORY-2.1.1](../stories/graphdocs/STORY-2.1.1-core-markdown-parser.md) | Core Parser (headers, paragraphs, lists, code) | Ready |
| [STORY-2.1.2](../stories/graphdocs/STORY-2.1.2-variable-detection.md) | Variable Detection & Typing | Ready |
| [STORY-2.1.3](../stories/graphdocs/STORY-2.1.3-template-conformance.md) | Template Conformance & Status Normalization | Ready |
| [STORY-2.1.4](../stories/graphdocs/STORY-2.1.4-agent-transformation.md) | Agent-Based Transformation (TEA subprocess) | Ready |

**Criterios de Aceitacao**:

*Core Parsing (2.1.1)*:
- [ ] AC1: Parse headers (H1-H6) como secoes
- [ ] AC2: Parse paragrafos como secoes
- [ ] AC3: Parse listas como secoes
- [ ] AC4: Parse code blocks como secoes

*Variable Detection (2.1.2)*:
- [ ] AC5: Detectar variaveis `{{name}}` com inferencia de tipo
- [ ] AC6: Suportar tipos: `bool`, `enum`, `number`, `string`, `string[]`, `object`
- [ ] AC7: Parse YAML frontmatter para type hints

*Template Conformance (2.1.3)*:
- [ ] AC8: Gerar estrutura de edges
- [ ] AC9: Detectar templates em diretorios e validar conformidade
- [ ] AC10: Normalizar status usando embeddings (model2vec)

*Agent Transformation (2.1.4)*:
- [ ] AC11: Usar YAML agents locais (TEA subprocess) com modelo GGUF para transformar documentos

**Arquitetura**:
```
2.1.1 (Core) ──┬──▶ 2.1.3 (Conformance) ──▶ 2.1.4 (Agent) ──▶ TEA (external)
               │
2.1.2 (Vars) ──┘
```

---

#### STORY-2.2: LLM Schema Converter
**Como** desenvolvedor
**Quero** converter texto livre em schema estruturado via LLM
**Para** popular grafos automaticamente

**Criterios de Aceitacao**:
- [ ] Prompt template para extracao de estrutura
- [ ] Validacao do output do LLM
- [ ] Fallback para parser deterministico

---

#### STORY-2.3: Import CLI
**Como** operador
**Quero** importar documentos Markdown existentes
**Para** migrar documentacao

**Criterios de Aceitacao**:
- [ ] `agentfs graphdocs import <file.md>`
- [ ] `agentfs graphdocs import-dir <dir>`
- [ ] Opcao `--llm` para usar LLM
- [ ] Opcao `--template` para heranca

---

### Fase 3: Rendering Engine

#### STORY-3.1: GraphDocsEngine
**Como** desenvolvedor
**Quero** um engine de renderizacao
**Para** gerar Markdown a partir do grafo

**Criterios de Aceitacao**:
- [ ] Resolver heranca de templates
- [ ] Carregar variaveis (com heranca)
- [ ] Ordenar secoes por `order_idx`
- [ ] Substituir `{{variable}}` por valores
- [ ] Gerar Markdown formatado

**Estrutura**:
```rust
pub struct GraphDocsEngine {
    pool: DuckConnectionPool,
}

impl GraphDocsEngine {
    pub async fn render(&self, doc_id: &str) -> Result<String>;
    pub async fn render_at(&self, doc_id: &str, event_id: i64) -> Result<String>;
    pub async fn get_variables(&self, doc_id: &str) -> Result<HashMap<String, Value>>;
    pub async fn set_variable(&self, doc_id: &str, name: &str, value: Value) -> Result<()>;
}
```

---

#### STORY-3.2: Heranca de Templates
**Como** autor de documentos
**Quero** criar documentos que herdam de templates
**Para** reutilizar estrutura comum

**Criterios de Aceitacao**:
- [ ] Documento filho herda secoes do pai
- [ ] Secoes podem ser sobrescritas por `source_section`
- [ ] Variaveis herdadas podem ser sobrescritas
- [ ] Cadeia de heranca ilimitada
- [ ] Deteccao de ciclos

**Exemplo**:
```sql
-- Template base
INSERT INTO gd_documents (id, title) VALUES ('readme-template', 'README Template');
INSERT INTO gd_sections (id, document_id, section_type, order_idx, content)
VALUES ('s1', 'readme-template', 'heading', 0, '# {{project_name}}');

-- Documento filho
INSERT INTO gd_documents (id, title, base_template)
VALUES ('my-readme', 'My README', 'readme-template');
INSERT INTO gd_variables (id, document_id, name, value)
VALUES ('v1', 'my-readme', 'project_name', '"My Awesome Project"');
```

---

#### STORY-3.3: Time-Travel Rendering
**Como** autor de documentos
**Quero** renderizar versoes anteriores
**Para** ver historico de documentos

**Criterios de Aceitacao**:
- [ ] `render_at(doc_id, event_id)` renderiza versao historica
- [ ] Usa fs_journal filtrado por event_id
- [ ] CLI: `agentfs graphdocs render <doc> --at <event_id>`

---

### Fase 4: FUSE Handler

#### STORY-4.1: GraphDocsHandler
**Como** desenvolvedor
**Quero** um handler FUSE para GraphDocs
**Para** renderizar documentos transparentemente

**Criterios de Aceitacao**:
- [x] Implementa `FileHandler` trait
- [x] Intercepta arquivos `.gd.md`
- [x] Renderiza documento do grafo no `read()`
- [x] Retorna stats virtuais no `getattr()`
- [x] Write retorna erro (read-only)

**Arquivo**: `cli/src/handler.rs` (struct `GraphDocsHandler`)

---

#### STORY-4.2: Virtual Directory Listing
**Como** usuario
**Quero** ver documentos GraphDocs em `ls`
**Para** descobrir documentos disponiveis

**Criterios de Aceitacao**:
- [ ] Handler retorna lista de documentos no `readdir()`
- [ ] Documentos aparecem como arquivos `.gd.md`
- [ ] Stats refletem tamanho renderizado

---

### Fase 5: CLI e Gestao

#### STORY-5.1: CLI GraphDocs
**Como** operador
**Quero** comandos para gerenciar GraphDocs
**Para** criar e editar documentos

**Criterios de Aceitacao**:
- [ ] `agentfs graphdocs create <doc_id> --title "Title"`
- [ ] `agentfs graphdocs add-section <doc_id> --type heading --content "# Title"`
- [ ] `agentfs graphdocs set-var <doc_id> <name> <value>`
- [ ] `agentfs graphdocs render <doc_id>`
- [ ] `agentfs graphdocs list`

---

#### STORY-5.2: Editor Integration
**Como** autor
**Quero** editar grafos via interface amigavel
**Para** nao precisar escrever SQL

**Criterios de Aceitacao**:
- [ ] Abrir editor de texto com YAML/TOML do documento
- [ ] Parse e update do grafo ao salvar
- [ ] Validacao de estrutura

---

## Modelo de Dados

### Documento
```yaml
document:
  id: "project-readme"
  title: "Project README"
  base_template: "readme-template"  # opcional
  language: "en"
  version: 1
```

### Secao
```yaml
section:
  id: "s1"
  document_id: "project-readme"
  parent_id: null  # ou ID de secao pai
  section_type: "heading"  # heading, paragraph, list, code, table
  level: 1  # para headings
  order_idx: 0
  content: "# {{project_name}}"
  condition: null  # opcional: variavel booleana
  is_inherited: false
```

### Variavel
```yaml
variable:
  id: "v1"
  document_id: "project-readme"
  name: "project_name"
  value: "My Project"
  var_type: "string"
  is_inherited: false
```

## Fluxo de Renderizacao

```
1. Requisicao: read("/docs/my-doc.gd.md")
           |
           v
2. GraphDocsHandler.can_handle() -> true
           |
           v
3. GraphDocsHandler.read()
           |
           v
4. Extrair doc_id: "my-doc"
           |
           v
5. GraphDocsEngine.render("my-doc")
           |
           +---> Carregar documento
           |        |
           |        v
           +---> Resolver heranca (recursivo)
           |        |
           |        v
           +---> Carregar secoes (merge)
           |        |
           |        v
           +---> Carregar variaveis (merge)
           |        |
           |        v
           +---> Ordenar secoes por order_idx
           |        |
           |        v
           +---> Renderizar cada secao
           |        |
           |        v
           +---> Substituir {{variaveis}}
           |        |
           |        v
           +---> Concatenar markdown
                    |
                    v
6. Retornar bytes do markdown
```

## Exemplo Completo

### Setup Inicial
```sql
-- Template README
INSERT INTO gd_documents (id, title)
VALUES ('readme-tmpl', 'README Template');

INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content)
VALUES
  ('t1', 'readme-tmpl', 'heading', 1, 0, '# {{project_name}}'),
  ('t2', 'readme-tmpl', 'paragraph', 0, 1, '{{description}}'),
  ('t3', 'readme-tmpl', 'heading', 2, 2, '## Installation'),
  ('t4', 'readme-tmpl', 'code', 0, 3, '```bash\n{{install_cmd}}\n```'),
  ('t5', 'readme-tmpl', 'heading', 2, 4, '## License'),
  ('t6', 'readme-tmpl', 'paragraph', 0, 5, '{{license}}');

-- Documento Concreto
INSERT INTO gd_documents (id, title, base_template)
VALUES ('agentfs-readme', 'AgentFS README', 'readme-tmpl');

INSERT INTO gd_variables (id, document_id, name, value)
VALUES
  ('v1', 'agentfs-readme', 'project_name', '"AgentFS"'),
  ('v2', 'agentfs-readme', 'description', '"A filesystem for AI agents"'),
  ('v3', 'agentfs-readme', 'install_cmd', '"pip install agentfs-sdk"'),
  ('v4', 'agentfs-readme', 'license', '"MIT License"');
```

### Resultado Renderizado
```markdown
# AgentFS

A filesystem for AI agents

## Installation

```bash
pip install agentfs-sdk
```

## License

MIT License
```

## Arquivos Relacionados

| Arquivo | Descricao | Status |
|---------|-----------|--------|
| `schema/duckagentfs.sql` | Tabelas gd_* | Criado |
| `cli/src/handler.rs` | GraphDocsHandler | Criado |
| `sdk/rust/src/graphdocs/engine.rs` | Engine (futuro) | Pendente |
| `sdk/rust/src/graphdocs/parser.rs` | Core Parser (2.1.1) | Pendente |
| `sdk/rust/src/graphdocs/variable_types.rs` | Variable Types (2.1.2) | Pendente |
| `sdk/rust/src/graphdocs/conformance.rs` | Template Conformance (2.1.3) | Pendente |
| `sdk/rust/src/graphdocs/normalizer.rs` | Status Normalization (2.1.3) | Pendente |
| `sdk/rust/src/graphdocs/embedding_matcher.rs` | Embedding Matcher (2.1.3) | Pendente |
| `sdk/rust/src/graphdocs/agent_transformer.rs` | TEA Subprocess (2.1.4) | Pendente |
| `agents/document-conformance-agent.yaml` | Status Agent (2.1.4) | Pendente |
| `agents/document-transformer-agent.yaml` | Transform Agent (2.1.4) | Pendente |

## Dependencias

```
EPIC-DUCKAGENTFS-001
    |
    +-- Fase 5 (FUSE + Handler Registry)
          |
          +-- EPIC-GRAPHDOCS-001
                |
                +-- GraphDocsHandler registrado no HandlerRegistry
                |
                +-- STORY-2.1.4 (Agent Transform)
                      |
                      +-- TEA Binary (external, subprocess)
                            |
                            +-- GGUF Model (gemma-3n-E4B-it)
```

### Dependencias Externas (STORY-2.1.4)

| Dependencia | Instalacao |
|-------------|------------|
| TEA binary | `cargo install --path /path/to/tea --features llm-local` |
| GGUF model | Download `gemma-3n-E4B-it-Q4_K_M.gguf` from HuggingFace |

## Riscos

| Risco | Probabilidade | Impacto | Mitigacao |
|-------|---------------|---------|-----------|
| Performance de renderizacao | Media | Medio | Cache de documentos renderizados |
| Complexidade de heranca | Media | Baixo | Limitar profundidade de heranca |
| Ciclos em grafos | Baixa | Alto | Deteccao de ciclos no engine |
