# EPIC-DUCKAGENTFS-001: DuckAgentFS - DuckDB Storage Backend

> **NOTA**: Esta documentacao e conceitual. Durante a fase de implementacao, alteracoes podem ser feitas para adaptar aos requisitos reais e especificidades das bibliotecas DuckDB.

## Visao Geral

| Campo | Valor |
|-------|-------|
| **ID** | EPIC-DUCKAGENTFS-001 |
| **Titulo** | DuckAgentFS - Backend de Armazenamento DuckDB |
| **Status** | Draft |
| **Prioridade** | Alta |
| **Estimativa** | - |

## Descricao

Implementar um backend de armazenamento para AgentFS usando DuckDB como banco de dados, aproveitando suas capacidades OLAP para analytics, extensoes VSS (Vector Similarity Search) para busca semantica, e DuckPGQ para analise de grafos de dependencias de codigo.

### Justificativa da Migracao SQLite -> DuckDB

1. **Performance Analitica**: DuckDB e otimizado para queries analiticas (OLAP) vs SQLite (OLTP)
2. **Extensoes Nativas**: VSS e PGQ sao extensoes oficiais do DuckDB
3. **Modelo Append-Only**: Permite time-travel e audit trail completo
4. **Columnar Storage**: Melhor compressao e performance para leitura

### Diferencas Arquiteturais

| Aspecto | AgentFS (SQLite) | DuckAgentFS (DuckDB) |
|---------|------------------|----------------------|
| Modelo de Dados | Mutacao direta | Append-only journal |
| Time-Travel | Nao suportado | Via event_id filter |
| Busca Semantica | Nao nativo | VSS extension |
| Grafos | Nao nativo | DuckPGQ extension |
| Concorrencia | WAL | Single Writer |

## Stories

### Fase 1: Core Storage Engine

#### STORY-1.1: Schema DDL DuckDB
**Como** desenvolvedor
**Quero** um schema DDL completo para DuckDB
**Para** ter a base de dados estruturada para o DuckAgentFS

**Criterios de Aceitacao**:
- [x] Tabela `fs_journal` com modelo append-only
- [x] View `fs_current` derivando estado atual
- [x] Tabela `fs_data` para chunks de dados
- [x] Sequences para inode e event allocation
- [x] Tabela `kv_store` para key-value
- [x] Tabela `tool_calls` para tracking

**Arquivo**: `schema/duckagentfs.sql`

---

#### STORY-1.2: DuckAgentFS FileSystem Trait
**Como** desenvolvedor
**Quero** uma implementacao do trait `FileSystem` para DuckDB
**Para** usar DuckAgentFS como backend transparente

**Criterios de Aceitacao**:
- [x] Implementar todos os metodos do trait `FileSystem`
- [x] Usar journal model para todas as mutacoes
- [x] Manter DentryCache para performance
- [x] Suportar symlinks e hardlinks
- [ ] Testes unitarios e de integracao

**Arquivo**: `sdk/rust/src/filesystem/duckagentfs.rs`

---

#### STORY-1.3: Connection Pool DuckDB
**Como** desenvolvedor
**Quero** um pool de conexoes otimizado para DuckDB
**Para** gerenciar concorrencia com single-writer semantics

**Criterios de Aceitacao**:
- [ ] Semaforo para operacoes de escrita
- [ ] Pool de conexoes de leitura
- [ ] Metricas de uso do pool
- [ ] Timeout configuravel

**Dependencias**: STORY-1.1

---

#### STORY-1.4: Time-Travel Queries
**Como** desenvolvedor
**Quero** consultar o filesystem em qualquer ponto no tempo
**Para** audit trail e recuperacao de dados

**Criterios de Aceitacao**:
- [x] Metodo `snapshot_at(event_id)` retorna filesystem read-only
- [x] View `fs_current` filtra por event_id
- [ ] API: `list_events(limit, offset)` para timeline
- [ ] API: `diff(from_event, to_event)` para comparacao
- [ ] Testes unitarios e de integracao

**Nota**: CLI commands movidos para STORY-6.1 per SCP-2026-01-14

**Arquivo**: `sdk/rust/src/filesystem/duckagentfs.rs` (struct `DuckAgentFSSnapshot`)

---

### Fase 2: Vector Similarity Search (VSS)

#### STORY-2.1: Embedding Generator Trait
**Como** desenvolvedor
**Quero** um trait para geracao de embeddings
**Para** suportar diferentes provedores (OpenAI, local, etc)

**Criterios de Aceitacao**:
- [x] Trait `EmbeddingGenerator` com metodos async
- [x] Implementacao NoOp para testes
- [x] Implementacao OpenAI (conceitual)
- [x] Implementacao Local/ONNX (conceitual)
- [x] Chunked embedding strategy

**Arquivo**: `sdk/rust/src/embedding.rs`

---

#### STORY-2.2: Schema VSS
**Como** desenvolvedor
**Quero** tabelas para armazenar embeddings
**Para** indexar arquivos semanticamente

**Criterios de Aceitacao**:
- [x] Tabela `fs_embeddings` com vetor de embedding
- [x] Tabela `fs_chunk_embeddings` para arquivos grandes
- [x] Index HNSW (comentado, requer extensao)

**Arquivo**: `schema/duckagentfs.sql`

---

#### STORY-2.3: Semantic Search API
**Como** desenvolvedor
**Quero** uma API de busca semantica
**Para** encontrar arquivos por significado

**Criterios de Aceitacao**:
- [x] Metodo `search(query, limit)` em DuckAgentFS
- [ ] CLI: `agentfs search "query"`
- [ ] Retornar path, score, preview

**Arquivo**: `sdk/rust/src/filesystem/duckagentfs.rs`

---

### Fase 3: Property Graphs (DuckPGQ)

#### STORY-3.1: Schema Code Graph
**Como** desenvolvedor
**Quero** tabelas para o grafo de dependencias de codigo
**Para** analisar relacoes entre simbolos

**Criterios de Aceitacao**:
- [x] Tabela `code_symbols` (funcoes, classes, modulos)
- [x] Tabela `code_dependencies` (calls, imports, extends)
- [x] Property Graph `code_graph` (comentado)

**Arquivo**: `schema/duckagentfs.sql`

---

#### STORY-3.2: Code Analyzer Integration
**Como** desenvolvedor
**Quero** integrar com analisadores de codigo
**Para** popular o grafo automaticamente

**Criterios de Aceitacao**:
- [ ] Trait `CodeAnalyzer` para diferentes linguagens
- [ ] Implementacao para Rust (tree-sitter)
- [ ] Implementacao para TypeScript
- [ ] Hooks de write_file para atualizar grafo

---

#### STORY-3.3: Graph Query API
**Como** desenvolvedor
**Quero** APIs para consultar o grafo de codigo
**Para** navegar dependencias

**Criterios de Aceitacao**:
- [ ] `get_callers(symbol)`: quem chama este simbolo
- [ ] `get_callees(symbol)`: quem este simbolo chama
- [ ] `get_dependencies(file)`: dependencias de um arquivo
- [ ] `get_impact(symbol)`: impacto de mudanca

---

### Fase 4: MCP Server Integration

#### STORY-4.1: MCP Tools para DuckAgentFS
**Como** agente AI
**Quero** ferramentas MCP para DuckAgentFS
**Para** interagir via protocolo padrao

**Criterios de Aceitacao**:
- [ ] Tool `duckagentfs_search`: busca semantica
- [ ] Tool `duckagentfs_snapshot`: time-travel
- [ ] Tool `duckagentfs_graph_query`: consulta de grafos
- [ ] Tool `duckagentfs_analyze`: analise de impacto

---

### Fase 5: FUSE + Handler Registry

#### STORY-5.1: FileHandler Trait
**Como** desenvolvedor
**Quero** um sistema de handlers extensivel
**Para** interceptar operacoes FUSE

**Criterios de Aceitacao**:
- [x] Trait `FileHandler` com metodos async
- [x] Suporte a prioridades
- [x] Metodos: can_handle, read, getattr, write, readdir

**Arquivo**: `cli/src/handler.rs`

---

#### STORY-5.2: HandlerRegistry
**Como** desenvolvedor
**Quero** um registro de handlers
**Para** gerenciar multiplos handlers por prioridade

**Criterios de Aceitacao**:
- [x] Registro/desregistro dinamico
- [x] Ordenacao por prioridade
- [x] Fallback para DefaultHandler
- [x] Dispatch para operacoes FUSE

**Arquivo**: `cli/src/handler.rs`

---

#### STORY-5.3: DefaultHandler
**Como** desenvolvedor
**Quero** um handler padrao
**Para** delegar ao FileSystem quando nenhum handler match

**Criterios de Aceitacao**:
- [x] Implementa FileHandler
- [x] Delega todas operacoes ao FileSystem
- [x] Prioridade maxima (ultimo a ser tentado)

**Arquivo**: `cli/src/handler.rs`

---

#### STORY-5.4: Integracao FUSE
**Como** desenvolvedor
**Quero** integrar HandlerRegistry no fuse.rs
**Para** usar handlers em operacoes FUSE

**Criterios de Aceitacao**:
- [ ] Adicionar `handler_registry` ao `AgentFSFuse`
- [ ] Modificar `read()` para usar handler_registry
- [ ] Modificar `getattr()` para usar handler_registry
- [ ] Testes de integracao

**Dependencias**: STORY-5.1, STORY-5.2, STORY-5.3

---

### Fase 6: Operations

#### STORY-6.1: CLI Commands
**Como** operador
**Quero** comandos CLI para DuckAgentFS
**Para** gerenciar databases

**Criterios de Aceitacao**:
- [ ] `agentfs init --backend duckdb`
- [ ] `agentfs search <query>`
- [ ] `agentfs snapshot --at <event_id>`
- [ ] `agentfs graph deps <file>`

---

#### STORY-6.2: Migracao SQLite -> DuckDB [Won't Do]
**Como** operador
**Quero** migrar databases existentes
**Para** adotar DuckAgentFS

**Status**: Won't Do - Starting fresh with DuckAgentFS, no migration needed.

**Criterios de Aceitacao**:
- [ ] ~~Script de migracao~~
- [ ] ~~Validacao pos-migracao~~
- [ ] ~~Rollback em caso de erro~~

---

## Arquivos Criados

| Arquivo | Descricao | Status |
|---------|-----------|--------|
| `schema/duckagentfs.sql` | DDL completo DuckDB | Criado |
| `sdk/rust/src/filesystem/duckagentfs.rs` | Implementacao FileSystem | Criado |
| `sdk/rust/src/embedding.rs` | Trait de embeddings | Criado |
| `cli/src/handler.rs` | Sistema de handlers | Criado |

## Codigo Reutilizado do AgentFS

| Componente | Reuso | Notas |
|------------|-------|-------|
| `fuser/` | 100% | Biblioteca FUSE completa |
| `FileSystem` trait | 100% | Interface de filesystem |
| `File` trait | 100% | Interface de arquivo |
| `DentryCache` | 90% | Adaptado para DuckDB |
| `HostFS` | 100% | Filesystem do host |
| `OverlayFS` | 80% | Pode ser adaptado |
| Error handling | 100% | Tipos de erro |

## Diagrama de Arquitetura

```
+------------------+     +------------------+
|   CLI / FUSE     |     |   MCP Server     |
+--------+---------+     +--------+---------+
         |                        |
         v                        v
+------------------+     +------------------+
| HandlerRegistry  |<--->|  DuckAgentFS     |
+--------+---------+     +--------+---------+
         |                        |
         v                        v
+------------------+     +------------------+
| DefaultHandler   |     | DuckDB Engine    |
| GraphDocsHandler |     +--------+---------+
+------------------+              |
                                  v
                    +---------------------------+
                    |      DuckDB Database      |
                    +---------------------------+
                    | fs_journal | fs_data      |
                    | kv_store   | tool_calls   |
                    | code_symbols | code_deps  |
                    | fs_embeddings             |
                    +---------------------------+
                                  |
                    +-------------+-------------+
                    |             |             |
                    v             v             v
               +-------+    +-------+    +-------+
               |  VSS  |    |DuckPGQ|    |HTTPFS |
               +-------+    +-------+    +-------+
```

## Riscos e Mitigacoes

| Risco | Probabilidade | Impacto | Mitigacao |
|-------|---------------|---------|-----------|
| DuckDB Rust bindings imaturos | Media | Alto | Usar FFI se necessario |
| Single-writer bottleneck | Media | Medio | Buffer de escrita, batch commits |
| Tamanho do journal crescendo | Alta | Medio | Compactacao periodica |
| Latencia de embedding | Alta | Baixo | Cache, embedding assincrono |

## Referencias

- [DuckDB Documentation](https://duckdb.org/docs/)
- [DuckDB VSS Extension](https://duckdb.org/docs/extensions/vss)
- [DuckDB PGQ Extension](https://github.com/duckdb/duckdb_pgq)
- [AgentFS Codebase](https://github.com/tursodatabase/agentfs)
