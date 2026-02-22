-- RustedClaw memory backend schema with pgvector support.
-- Run: CREATE EXTENSION IF NOT EXISTS vector;  (before this migration)

CREATE TABLE IF NOT EXISTS memories (
    id          TEXT PRIMARY KEY,
    content     TEXT NOT NULL,
    tags        TEXT[] NOT NULL DEFAULT '{}',
    source      TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_accessed TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    score       REAL NOT NULL DEFAULT 0.0,
    embedding   vector(1536)  -- OpenAI ada-002 dimension; adjust as needed
);

-- Index for fast keyword search (GIN trigram)
CREATE INDEX IF NOT EXISTS idx_memories_content_trgm
    ON memories USING gin (content gin_trgm_ops);

-- Index for tag filtering
CREATE INDEX IF NOT EXISTS idx_memories_tags
    ON memories USING gin (tags);

-- Index for vector similarity search (IVFFlat for speed)
CREATE INDEX IF NOT EXISTS idx_memories_embedding
    ON memories USING ivfflat (embedding vector_cosine_ops)
    WITH (lists = 100);

-- Index for recency ordering
CREATE INDEX IF NOT EXISTS idx_memories_created_at
    ON memories (created_at DESC);
