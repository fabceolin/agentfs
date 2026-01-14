-- ============================================================================
-- DuckAgentFS - Complete DDL Schema
-- ============================================================================
-- Database schema for DuckAgentFS, an OLAP-oriented filesystem for AI agents
-- using DuckDB with append-only journal model.
--
-- Features:
--   - Append-only journal for time-travel and audit
--   - Vector Similarity Search (VSS) for semantic file search
--   - Property Graphs (DuckPGQ) for code dependencies and document structure
--   - HTTPFS support for hybrid local/S3 storage
-- ============================================================================

-- ============================================================================
-- PART 1: CORE FILESYSTEM (Append-Only Journal Model)
-- ============================================================================

-- Sequence for event IDs (must be created before fs_journal)
CREATE SEQUENCE IF NOT EXISTS fs_event_seq START 1;

-- Sequence for inode allocation
CREATE SEQUENCE IF NOT EXISTS fs_inode_seq START 2;  -- 1 is reserved for root

-- Journal of all filesystem events (append-only)
CREATE TABLE IF NOT EXISTS fs_journal (
    event_id    UBIGINT PRIMARY KEY DEFAULT nextval('fs_event_seq'),
    inode       UBIGINT NOT NULL,
    event_type  VARCHAR NOT NULL,  -- 'create', 'update', 'delete', 'rename', 'chmod'
    event_time  TIMESTAMP DEFAULT current_timestamp,

    -- Snapshot of inode state at this event
    parent      UBIGINT,
    name        VARCHAR,
    mode        UINTEGER,          -- Unix mode (file type + permissions)
    uid         UINTEGER DEFAULT 0,
    gid         UINTEGER DEFAULT 0,
    size        UBIGINT DEFAULT 0,
    nlink       UINTEGER DEFAULT 1,

    -- Extended attributes
    xattrs      JSON,

    -- For renames: old parent and name
    old_parent  UBIGINT,
    old_name    VARCHAR,

    -- Actor tracking
    actor_id    VARCHAR,           -- Agent or process that made the change
    session_id  VARCHAR,           -- Session context

    -- Metadata
    metadata    JSON
);

-- Current filesystem state view (derived from journal)
CREATE OR REPLACE VIEW fs_current AS
WITH ranked AS (
    SELECT
        *,
        ROW_NUMBER() OVER (PARTITION BY inode ORDER BY event_id DESC) as rn
    FROM fs_journal
    WHERE event_type != 'delete'
)
SELECT
    inode,
    parent,
    name,
    mode,
    uid,
    gid,
    size,
    nlink,
    xattrs,
    event_time as mtime,
    event_id as last_event_id,
    actor_id,
    session_id
FROM ranked
WHERE rn = 1
  AND inode NOT IN (
      SELECT inode FROM fs_journal WHERE event_type = 'delete'
      AND event_id = (SELECT MAX(event_id) FROM fs_journal j2 WHERE j2.inode = fs_journal.inode)
  );

-- File data storage (chunked for large files)
CREATE TABLE IF NOT EXISTS fs_data (
    inode       UBIGINT NOT NULL,
    chunk_idx   UINTEGER NOT NULL,
    data        BLOB NOT NULL,
    checksum    VARCHAR,           -- Optional integrity check
    created_at  TIMESTAMP DEFAULT current_timestamp,
    PRIMARY KEY (inode, chunk_idx)
);

-- Index for efficient inode lookups
CREATE INDEX IF NOT EXISTS idx_fs_journal_inode ON fs_journal(inode);
CREATE INDEX IF NOT EXISTS idx_fs_journal_parent ON fs_journal(parent);
CREATE INDEX IF NOT EXISTS idx_fs_journal_name ON fs_journal(name);
CREATE INDEX IF NOT EXISTS idx_fs_journal_event_time ON fs_journal(event_time);

-- Initialize root directory if not exists
INSERT INTO fs_journal (inode, event_type, parent, name, mode, nlink)
SELECT 1, 'create', 1, '', 16877, 2  -- 16877 = S_IFDIR | 0755
WHERE NOT EXISTS (SELECT 1 FROM fs_journal WHERE inode = 1);

-- ============================================================================
-- PART 2: KEY-VALUE STORE
-- ============================================================================

CREATE TABLE IF NOT EXISTS kv_store (
    key         VARCHAR PRIMARY KEY,
    value       JSON NOT NULL,
    created_at  TIMESTAMP DEFAULT current_timestamp,
    updated_at  TIMESTAMP DEFAULT current_timestamp,
    expires_at  TIMESTAMP,         -- Optional TTL
    version     UBIGINT DEFAULT 1,
    metadata    JSON
);

-- Index for prefix queries
CREATE INDEX IF NOT EXISTS idx_kv_store_key ON kv_store(key);

-- ============================================================================
-- PART 3: TOOL CALLS TRACKING
-- ============================================================================

CREATE TABLE IF NOT EXISTS tool_calls (
    id              VARCHAR PRIMARY KEY,
    name            VARCHAR NOT NULL,
    status          VARCHAR NOT NULL DEFAULT 'running',  -- 'running', 'success', 'error'
    started_at      TIMESTAMP NOT NULL,
    completed_at    TIMESTAMP,
    duration_ms     DOUBLE,
    parameters      JSON,
    result          JSON,
    error           VARCHAR,
    session_id      VARCHAR,
    parent_call_id  VARCHAR,       -- For nested tool calls
    metadata        JSON
);

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_tool_calls_name ON tool_calls(name);
CREATE INDEX IF NOT EXISTS idx_tool_calls_status ON tool_calls(status);
CREATE INDEX IF NOT EXISTS idx_tool_calls_started_at ON tool_calls(started_at);
CREATE INDEX IF NOT EXISTS idx_tool_calls_session ON tool_calls(session_id);

-- Tool statistics view
CREATE OR REPLACE VIEW tool_stats AS
SELECT
    name,
    COUNT(*) as total_calls,
    COUNT(*) FILTER (WHERE status = 'success') as successful,
    COUNT(*) FILTER (WHERE status = 'error') as failed,
    AVG(duration_ms) as avg_duration_ms,
    MIN(duration_ms) as min_duration_ms,
    MAX(duration_ms) as max_duration_ms,
    PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY duration_ms) as p50_duration_ms,
    PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY duration_ms) as p95_duration_ms,
    PERCENTILE_CONT(0.99) WITHIN GROUP (ORDER BY duration_ms) as p99_duration_ms
FROM tool_calls
WHERE status != 'running'
GROUP BY name;

-- ============================================================================
-- PART 4: VECTOR SIMILARITY SEARCH (VSS)
-- ============================================================================

-- File embeddings for semantic search
CREATE TABLE IF NOT EXISTS fs_embeddings (
    inode           UBIGINT PRIMARY KEY,
    embedding       FLOAT[1536],   -- OpenAI ada-002 dimension, configurable
    model           VARCHAR NOT NULL DEFAULT 'text-embedding-ada-002',
    content_hash    VARCHAR,       -- Hash of content used to generate embedding
    generated_at    TIMESTAMP DEFAULT current_timestamp,
    metadata        JSON
);

-- HNSW index for fast similarity search
-- Note: Requires VSS extension: INSTALL vss; LOAD vss;
-- CREATE INDEX IF NOT EXISTS idx_fs_embeddings_hnsw
-- ON fs_embeddings USING HNSW (embedding)
-- WITH (metric = 'cosine');

-- Chunk-level embeddings for large files
CREATE TABLE IF NOT EXISTS fs_chunk_embeddings (
    inode           UBIGINT NOT NULL,
    chunk_idx       UINTEGER NOT NULL,
    start_offset    UBIGINT NOT NULL,
    end_offset      UBIGINT NOT NULL,
    embedding       FLOAT[1536],
    content_preview VARCHAR(500),  -- First 500 chars for context
    generated_at    TIMESTAMP DEFAULT current_timestamp,
    PRIMARY KEY (inode, chunk_idx)
);

-- ============================================================================
-- PART 5: DUCKPGQ - PROPERTY GRAPHS
-- ============================================================================

-- ---------------------------------------------------------------------------
-- 5.1: CODE DEPENDENCY GRAPH
-- ---------------------------------------------------------------------------

-- Code symbols (functions, classes, modules, etc.)
CREATE TABLE IF NOT EXISTS code_symbols (
    id          VARCHAR PRIMARY KEY,  -- 'file:symbol_name' or unique ID
    inode       UBIGINT NOT NULL,
    name        VARCHAR NOT NULL,
    kind        VARCHAR NOT NULL,     -- 'function', 'class', 'module', 'variable', 'type'
    language    VARCHAR,
    start_line  UINTEGER,
    end_line    UINTEGER,
    signature   VARCHAR,              -- Function/method signature
    docstring   VARCHAR,
    visibility  VARCHAR DEFAULT 'public',  -- 'public', 'private', 'protected'
    metadata    JSON,
    FOREIGN KEY (inode) REFERENCES fs_current(inode)
);

-- Dependencies between symbols
CREATE TABLE IF NOT EXISTS code_dependencies (
    source_id   VARCHAR NOT NULL,
    target_id   VARCHAR NOT NULL,
    dep_type    VARCHAR NOT NULL,     -- 'calls', 'imports', 'extends', 'implements', 'uses'
    weight      DOUBLE DEFAULT 1.0,   -- Dependency strength
    metadata    JSON,
    PRIMARY KEY (source_id, target_id, dep_type),
    FOREIGN KEY (source_id) REFERENCES code_symbols(id),
    FOREIGN KEY (target_id) REFERENCES code_symbols(id)
);

-- Property Graph definition for code analysis
-- Note: Requires DuckPGQ extension: INSTALL duckpgq; LOAD duckpgq;
-- CREATE PROPERTY GRAPH code_graph
-- VERTEX TABLES (
--     code_symbols PROPERTIES (id, name, kind, language, signature) LABEL symbol
-- )
-- EDGE TABLES (
--     code_dependencies SOURCE KEY (source_id) REFERENCES code_symbols (id)
--                       DESTINATION KEY (target_id) REFERENCES code_symbols (id)
--                       PROPERTIES (dep_type, weight) LABEL depends_on
-- );

-- ---------------------------------------------------------------------------
-- 5.2: GRAPHDOCS - DOCUMENT STRUCTURE GRAPH
-- ---------------------------------------------------------------------------

-- Documents (markdown files rendered from graph)
CREATE TABLE IF NOT EXISTS gd_documents (
    id              VARCHAR PRIMARY KEY,
    inode           UBIGINT UNIQUE,    -- Virtual inode in filesystem
    title           VARCHAR NOT NULL,
    base_template   VARCHAR,           -- Parent template for inheritance
    language        VARCHAR DEFAULT 'en',
    version         UINTEGER DEFAULT 1,
    created_at      TIMESTAMP DEFAULT current_timestamp,
    updated_at      TIMESTAMP DEFAULT current_timestamp,
    metadata        JSON,
    FOREIGN KEY (base_template) REFERENCES gd_documents(id)
);

-- Document sections (nodes in the document graph)
CREATE TABLE IF NOT EXISTS gd_sections (
    id              VARCHAR PRIMARY KEY,
    document_id     VARCHAR NOT NULL,
    parent_id       VARCHAR,           -- Parent section for hierarchy
    section_type    VARCHAR NOT NULL,  -- 'heading', 'paragraph', 'list', 'code', 'table'
    level           UINTEGER DEFAULT 1,-- Heading level (1-6) or nesting depth
    order_idx       UINTEGER NOT NULL, -- Order within parent
    content         VARCHAR,           -- Raw content with {{variable}} placeholders
    condition       VARCHAR,           -- Optional: variable name for conditional rendering
    is_inherited    BOOLEAN DEFAULT FALSE,
    source_section  VARCHAR,           -- Original section if inherited
    metadata        JSON,
    FOREIGN KEY (document_id) REFERENCES gd_documents(id),
    FOREIGN KEY (parent_id) REFERENCES gd_sections(id),
    FOREIGN KEY (source_section) REFERENCES gd_sections(id)
);

-- Variables for template rendering
CREATE TABLE IF NOT EXISTS gd_variables (
    id              VARCHAR PRIMARY KEY,
    document_id     VARCHAR NOT NULL,
    name            VARCHAR NOT NULL,
    value           JSON,              -- Variable value (any JSON type)
    var_type        VARCHAR DEFAULT 'string',  -- 'string', 'number', 'boolean', 'array', 'object'
    description     VARCHAR,
    is_inherited    BOOLEAN DEFAULT FALSE,
    source_doc      VARCHAR,           -- Document that defined this variable
    created_at      TIMESTAMP DEFAULT current_timestamp,
    updated_at      TIMESTAMP DEFAULT current_timestamp,
    UNIQUE (document_id, name),
    FOREIGN KEY (document_id) REFERENCES gd_documents(id),
    FOREIGN KEY (source_doc) REFERENCES gd_documents(id)
);

-- Edges between sections and documents
CREATE TABLE IF NOT EXISTS gd_edges (
    source_id       VARCHAR NOT NULL,
    target_id       VARCHAR NOT NULL,
    edge_type       VARCHAR NOT NULL,  -- 'contains', 'references', 'extends', 'next'
    metadata        JSON,
    PRIMARY KEY (source_id, target_id, edge_type)
);

-- Property Graph definition for GraphDocs
-- CREATE PROPERTY GRAPH graphdocs
-- VERTEX TABLES (
--     gd_documents PROPERTIES (id, title, language, version) LABEL document,
--     gd_sections PROPERTIES (id, section_type, level, content) LABEL section,
--     gd_variables PROPERTIES (id, name, value, var_type) LABEL variable
-- )
-- EDGE TABLES (
--     gd_edges SOURCE KEY (source_id) REFERENCES gd_sections (id)
--              DESTINATION KEY (target_id) REFERENCES gd_sections (id)
--              PROPERTIES (edge_type) LABEL edge,
--     (SELECT document_id as source_id, id as target_id, 'has_section' as rel
--      FROM gd_sections)
--              SOURCE KEY (source_id) REFERENCES gd_documents (id)
--              DESTINATION KEY (target_id) REFERENCES gd_sections (id)
--              LABEL has_section,
--     (SELECT document_id as source_id, id as target_id, 'has_variable' as rel
--      FROM gd_variables)
--              SOURCE KEY (source_id) REFERENCES gd_documents (id)
--              DESTINATION KEY (target_id) REFERENCES gd_variables (id)
--              LABEL has_variable
-- );

-- Indexes for GraphDocs
CREATE INDEX IF NOT EXISTS idx_gd_sections_document ON gd_sections(document_id);
CREATE INDEX IF NOT EXISTS idx_gd_sections_parent ON gd_sections(parent_id);
CREATE INDEX IF NOT EXISTS idx_gd_variables_document ON gd_variables(document_id);
CREATE INDEX IF NOT EXISTS idx_gd_variables_name ON gd_variables(document_id, name);

-- ============================================================================
-- PART 6: HTTPFS REMOTE STORAGE SUPPORT
-- ============================================================================

-- Remote storage configuration
CREATE TABLE IF NOT EXISTS remote_storage (
    id              VARCHAR PRIMARY KEY,
    storage_type    VARCHAR NOT NULL,  -- 's3', 'http', 'gcs', 'azure'
    endpoint        VARCHAR NOT NULL,
    bucket          VARCHAR,
    prefix          VARCHAR,
    credentials     JSON,              -- Encrypted credentials
    is_active       BOOLEAN DEFAULT TRUE,
    priority        UINTEGER DEFAULT 100,
    created_at      TIMESTAMP DEFAULT current_timestamp,
    metadata        JSON
);

-- File remote locations (for files stored remotely)
CREATE TABLE IF NOT EXISTS fs_remote_refs (
    inode           UBIGINT PRIMARY KEY,
    storage_id      VARCHAR NOT NULL,
    remote_path     VARCHAR NOT NULL,
    remote_size     UBIGINT,
    remote_etag     VARCHAR,
    cached_locally  BOOLEAN DEFAULT FALSE,
    cache_expires   TIMESTAMP,
    last_synced     TIMESTAMP DEFAULT current_timestamp,
    FOREIGN KEY (storage_id) REFERENCES remote_storage(id)
);

-- ============================================================================
-- PART 7: SESSION AND AUDIT
-- ============================================================================

-- Sequence for audit log (must be created before audit_log table)
CREATE SEQUENCE IF NOT EXISTS audit_seq START 1;

-- Active sessions
CREATE TABLE IF NOT EXISTS sessions (
    id              VARCHAR PRIMARY KEY,
    agent_id        VARCHAR,
    started_at      TIMESTAMP DEFAULT current_timestamp,
    last_activity   TIMESTAMP DEFAULT current_timestamp,
    metadata        JSON
);

-- Audit log (separate from fs_journal for non-fs events)
CREATE TABLE IF NOT EXISTS audit_log (
    id              UBIGINT PRIMARY KEY DEFAULT nextval('audit_seq'),
    event_type      VARCHAR NOT NULL,
    event_time      TIMESTAMP DEFAULT current_timestamp,
    session_id      VARCHAR,
    actor_id        VARCHAR,
    resource_type   VARCHAR,
    resource_id     VARCHAR,
    action          VARCHAR,
    old_value       JSON,
    new_value       JSON,
    metadata        JSON
);

-- ============================================================================
-- PART 8: UTILITY FUNCTIONS AND VIEWS
-- ============================================================================

-- View: Directory contents with full path
CREATE OR REPLACE VIEW fs_tree AS
WITH RECURSIVE tree AS (
    SELECT
        inode,
        parent,
        name,
        mode,
        size,
        mtime,
        name as path,
        0 as depth
    FROM fs_current
    WHERE inode = 1

    UNION ALL

    SELECT
        c.inode,
        c.parent,
        c.name,
        c.mode,
        c.size,
        c.mtime,
        CASE WHEN t.path = '' THEN c.name ELSE t.path || '/' || c.name END as path,
        t.depth + 1
    FROM fs_current c
    JOIN tree t ON c.parent = t.inode
    WHERE c.inode != 1
)
SELECT * FROM tree;

-- View: File type helper
CREATE OR REPLACE VIEW fs_files AS
SELECT
    *,
    CASE
        WHEN (mode & 61440) = 32768 THEN 'file'
        WHEN (mode & 61440) = 16384 THEN 'directory'
        WHEN (mode & 61440) = 40960 THEN 'symlink'
        ELSE 'other'
    END as file_type
FROM fs_current;

-- View: Recent filesystem activity
CREATE OR REPLACE VIEW fs_recent_activity AS
SELECT
    event_id,
    inode,
    event_type,
    event_time,
    name,
    actor_id,
    session_id
FROM fs_journal
ORDER BY event_id DESC
LIMIT 100;

-- View: GraphDocs rendered (basic, without variable substitution)
CREATE OR REPLACE VIEW gd_rendered_sections AS
SELECT
    d.id as document_id,
    d.title as document_title,
    s.id as section_id,
    s.section_type,
    s.level,
    s.order_idx,
    s.content,
    s.condition
FROM gd_documents d
JOIN gd_sections s ON s.document_id = d.id
ORDER BY d.id, s.order_idx;

-- ============================================================================
-- PART 9: SNAPSHOT FUNCTIONS (for time-travel)
-- ============================================================================

-- Note: These are template functions. In DuckDB, you'd use parameterized views
-- or implement these in application code.

-- Example: Get filesystem state at a specific event
-- SELECT * FROM fs_journal WHERE event_id <= $snapshot_event_id
-- Then apply the same fs_current logic with that filter.

-- ============================================================================
-- PART 10: MAINTENANCE
-- ============================================================================

-- Vacuum and analyze should be run periodically
-- VACUUM ANALYZE;

-- For append-only tables, consider periodic compaction:
-- CREATE TABLE fs_journal_archive AS SELECT * FROM fs_journal WHERE event_time < current_date - INTERVAL '90 days';
-- DELETE FROM fs_journal WHERE event_time < current_date - INTERVAL '90 days';
