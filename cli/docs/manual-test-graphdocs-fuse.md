# Manual Testing: GraphDocs FUSE Mount

This guide covers how to manually test the GraphDocs virtual directory feature via FUSE mount.

## Prerequisites

1. Linux system with FUSE support
2. `fusermount` installed (usually part of `fuse` package)
3. Built CLI: `cargo build --release` from `cli/` directory
4. A DuckDB database with GraphDocs tables

## Setup Test Database

### Option 1: Use the Demo Database

The demo database at `cli/demo/demo.duckdb` already has GraphDocs tables and sample documents.

### Option 2: Create a Fresh Database

```bash
# Create a new DuckDB database with GraphDocs schema
duckdb /tmp/test-graphdocs.duckdb << 'EOF'
-- Create GraphDocs tables
CREATE TABLE gd_documents(
    id VARCHAR PRIMARY KEY,
    inode UBIGINT UNIQUE,
    title VARCHAR NOT NULL,
    base_template VARCHAR,
    language VARCHAR DEFAULT('en'),
    version UINTEGER DEFAULT(1),
    created_at TIMESTAMP DEFAULT(current_timestamp),
    updated_at TIMESTAMP DEFAULT(current_timestamp),
    metadata JSON
);

CREATE TABLE gd_sections(
    id VARCHAR PRIMARY KEY,
    document_id VARCHAR NOT NULL REFERENCES gd_documents(id),
    name VARCHAR NOT NULL,
    section_type VARCHAR NOT NULL,
    content TEXT,
    ordering UINTEGER DEFAULT(0),
    UNIQUE(document_id, name)
);

CREATE TABLE gd_variables(
    id VARCHAR PRIMARY KEY,
    document_id VARCHAR NOT NULL REFERENCES gd_documents(id),
    name VARCHAR NOT NULL,
    value JSON,
    description VARCHAR,
    var_type VARCHAR DEFAULT('string'),
    UNIQUE(document_id, name)
);

-- Create filesystem tables (required by DuckAgentFS)
CREATE SEQUENCE IF NOT EXISTS fs_event_seq START 1;
CREATE SEQUENCE IF NOT EXISTS fs_inode_seq START 2;

CREATE TABLE IF NOT EXISTS fs_journal (
    event_id UBIGINT PRIMARY KEY DEFAULT nextval('fs_event_seq'),
    inode UBIGINT NOT NULL,
    event_type VARCHAR NOT NULL,
    event_time TIMESTAMP DEFAULT current_timestamp,
    parent UBIGINT,
    name VARCHAR,
    mode UINTEGER,
    uid UINTEGER DEFAULT 0,
    gid UINTEGER DEFAULT 0,
    size UBIGINT DEFAULT 0,
    nlink UINTEGER DEFAULT 1,
    xattrs JSON,
    old_parent UBIGINT,
    old_name VARCHAR,
    actor_id VARCHAR,
    session_id VARCHAR,
    metadata JSON
);

CREATE OR REPLACE VIEW fs_current AS
WITH ranked AS (
    SELECT *, ROW_NUMBER() OVER (PARTITION BY inode ORDER BY event_id DESC) as rn
    FROM fs_journal WHERE event_type != 'delete'
)
SELECT inode, parent, name, mode, uid, gid, size, nlink, xattrs,
       event_time as mtime, event_id as last_event_id, actor_id, session_id
FROM ranked WHERE rn = 1;

CREATE TABLE IF NOT EXISTS fs_data (
    inode UBIGINT NOT NULL,
    chunk_idx UINTEGER NOT NULL,
    data BLOB NOT NULL,
    checksum VARCHAR,
    created_at TIMESTAMP DEFAULT current_timestamp,
    PRIMARY KEY (inode, chunk_idx)
);

-- Initialize root directory
INSERT INTO fs_journal (inode, event_type, parent, name, mode, nlink)
VALUES (1, 'create', 1, '', 16877, 2);

-- Add sample GraphDocs documents
INSERT INTO gd_documents (id, title) VALUES
    ('readme', 'Project README'),
    ('api-docs', 'API Documentation');

INSERT INTO gd_sections (id, document_id, name, section_type, content, ordering) VALUES
    ('s1', 'readme', 'Introduction', 'Text', '# Project README

Welcome to the project!

Project: {{ project_name }}
Version: {{ version }}
', 1),
    ('s2', 'api-docs', 'Overview', 'Text', '# API Documentation

Endpoint: {{ api_endpoint }}
', 1);

INSERT INTO gd_variables (id, document_id, name, value, var_type) VALUES
    ('v1', 'readme', 'project_name', '"My Awesome Project"', 'string'),
    ('v2', 'readme', 'version', '"1.0.0"', 'string'),
    ('v3', 'api-docs', 'api_endpoint', '"https://api.example.com"', 'string');

CHECKPOINT;
EOF
```

## Test Procedure

### Step 1: Create Mount Point

```bash
mkdir -p /tmp/agentfs-mount
```

### Step 2: Mount the Database

```bash
# Using the demo database
./target/release/agentfs mount cli/demo/demo.duckdb /tmp/agentfs-mount --foreground

# Or using your test database
./target/release/agentfs mount /tmp/test-graphdocs.duckdb /tmp/agentfs-mount --foreground
```

**Expected output:**
```
INFO agentfs::cmd::mount: GraphDocs tables detected, registering handlers
INFO agentfs::fuser::session: Mounting /tmp/agentfs-mount
```

### Step 3: Test Directory Listing

In a new terminal:

```bash
# List root directory - should show .graphdocs
ls -la /tmp/agentfs-mount/
```

**Expected output:**
```
drwxr-xr-x  2 user user 4096 ... .
drwxrwxrwt 88 root root 3140 ... ..
dr-xr-xr-x  2 user user 4096 ... .graphdocs
```

### Step 4: Test GraphDocs Directory

```bash
# List GraphDocs virtual directory
ls -la /tmp/agentfs-mount/.graphdocs/
```

**Expected output:**
```
dr-xr-xr-x 2 user user 4096 ... .
drwxr-xr-x 2 user user 4096 ... ..
-r--r--r-- 1 user user  XXX ... readme.gd.md
-r--r--r-- 1 user user  XXX ... api-docs.gd.md
```

### Step 5: Test Document Reading

```bash
# Read a rendered document
cat /tmp/agentfs-mount/.graphdocs/readme.gd.md
```

**Expected output (variables should be substituted):**
```markdown
# Project README

Welcome to the project!

Project: My Awesome Project
Version: 1.0.0
```

### Step 6: Test File Stats

```bash
# Check file attributes
stat /tmp/agentfs-mount/.graphdocs/readme.gd.md
```

**Expected output:**
- Valid inode (large number like 9223372036854775806)
- Size matching rendered content
- Permissions: 0444 (read-only)

### Step 7: Test with Debug Logging

For troubleshooting, run with debug logging:

```bash
RUST_LOG=agentfs=debug ./target/release/agentfs mount \
    cli/demo/demo.duckdb /tmp/agentfs-mount --foreground 2>&1 | tee /tmp/fuse-debug.log
```

## Cleanup

```bash
# Unmount the filesystem
fusermount -u /tmp/agentfs-mount

# Remove mount point
rmdir /tmp/agentfs-mount
```

## Troubleshooting

### "Transport endpoint is not connected"

The mount point is in a bad state. Clean up:

```bash
fusermount -u /tmp/agentfs-mount
rm -rf /tmp/agentfs-mount
mkdir -p /tmp/agentfs-mount
```

### "Mountpoint does not exist"

Create the mount point directory:

```bash
mkdir -p /tmp/agentfs-mount
```

### ".graphdocs shows as d?????????"

This indicates lookup is failing. Check:
1. Database has `gd_documents` table
2. Run with `RUST_LOG=agentfs=debug` to see handler dispatch

### "No such file or directory" when reading

Check the FUSE debug log for:
- `handler-managed virtual file` message in open
- `Handler 'graphdocs' handled read` message

### Database Lock Errors

Copy the database to a temporary location:

```bash
cp cli/demo/demo.duckdb /tmp/test-mount.duckdb
./target/release/agentfs mount /tmp/test-mount.duckdb /tmp/agentfs-mount --foreground
```

## Verification Checklist

- [ ] `ls /tmp/agentfs-mount/` shows `.graphdocs` directory
- [ ] `ls /tmp/agentfs-mount/.graphdocs/` lists all documents as `{id}.gd.md`
- [ ] `cat /tmp/agentfs-mount/.graphdocs/{id}.gd.md` renders document with variables
- [ ] `stat` shows valid inode and file size for documents
- [ ] Mount starts with "GraphDocs tables detected" message
- [ ] Unmount completes cleanly with `fusermount -u`
