# Ferrum AI MVP v1

Local single-user lab to orchestrate `codex` and `claude` CLI sessions from a browser UI, persist chats/runs/usage in PostgreSQL, and leave the schema ready for later RAG and BI work.

## What is here

- `crates/orchestrator-core`: reusable command building and stream normalization for Codex/Claude.
- `apps/gateway`: Axum gateway with PostgreSQL persistence, login/run orchestration, SSE streams, and static file serving.
- `apps/web-lab`: React + Vite frontend-lab (dark minimal UI, providers page, paginated chats, collapsible event drawers).
- `docker-compose.yml`: local Postgres with `pgvector`.

## Prerequisites

- Rust 1.93+
- Docker / Docker Compose
- `codex` CLI installed locally
- `claude` CLI installed locally
- On Windows, Claude Code needs Git Bash available or `CLAUDE_CODE_GIT_BASH_PATH` set

## Run

1. Start PostgreSQL:

```bash
docker compose up -d
```

2. Copy env values if needed:

```bash
cp .env.example .env
```

3. Build the frontend:

```bash
cd apps/web-lab
npm.cmd install
npm.cmd run build
cd ../..
```

4. Run the gateway:

```bash
cargo run -p gateway
```

5. Open:

```text
http://127.0.0.1:3000
```

Detailed local setup:

- [docs/runbooks/quickstart.md](C:/Users/fraan/proyectos/ferrum-ai/docs/runbooks/quickstart.md)

## API highlights

- `GET /api/providers`
- `GET /api/providers/{provider}/preferences`
- `PUT /api/providers/{provider}/preferences`
- `POST /api/providers/{provider}/login`
- `POST /api/providers/{provider}/logout`
- `GET /api/providers/{provider}/auth-stream/{auth_id}`
- `GET /api/chats`
- `POST /api/chats`
- `GET /api/chats/{chat_id}/messages`
- `POST /api/chats/{chat_id}/messages`
- `GET /api/runs/{run_id}/stream`
- `GET /api/usage/summary`

## Notes

- Chat continuity is based on provider session ids, not a permanently alive PTY.
- Normal runs use subprocess streaming with structured parsing.
- The frontend is intentionally a lab UI; the reusable base is the core crate plus the gateway.
- `documents` and `chunks` tables are created now, but embeddings/RAG are deferred.
