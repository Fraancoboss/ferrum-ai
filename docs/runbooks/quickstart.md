# Quickstart

This runbook covers the full local setup for Ferrum AI MVP v1, from cloning the repository to opening the frontend-lab in a browser.

## 1. Clone the repository

### PowerShell

```powershell
git clone <YOUR_REPO_URL> ferrum-ai
Set-Location ferrum-ai
```

### Bash

```bash
git clone <YOUR_REPO_URL> ferrum-ai
cd ferrum-ai
```

## 2. Install prerequisites

Required:

- Rust 1.93 or newer
- Cargo
- Docker Desktop or Docker Engine with Compose
- Codex CLI installed locally
- Claude Code installed locally

Windows-specific:

- Claude Code requires Git Bash available in `PATH`, or `CLAUDE_CODE_GIT_BASH_PATH` pointing to `bash.exe`

Recommended verification:

### PowerShell

```powershell
rustc --version
cargo --version
docker --version
docker compose version
codex.cmd --version
claude --version
```

### Bash

```bash
rustc --version
cargo --version
docker --version
docker compose version
codex --version
claude --version
```

## 3. Create local environment config

Copy the sample env file.

### PowerShell

```powershell
Copy-Item .env.example .env
```

### Bash

```bash
cp .env.example .env
```

Default values are enough for a first local run:

```dotenv
BIND_ADDR=127.0.0.1:3000
DATABASE_URL=postgres://chatbot:chatbot@127.0.0.1:5433/chatbot
WORKSPACE_DIR=.
FRONTEND_DIR=apps/web-lab
CODEX_DAILY_SOFT_LIMIT_TOKENS=500000
CLAUDE_DAILY_SOFT_LIMIT_TOKENS=500000
```

Use:

```dotenv
FRONTEND_DIR=apps/web-lab/dist
```

## 4. Start PostgreSQL with pgvector

```powershell
docker compose up -d
```

Verify the container is up:

```powershell
docker compose ps
```

Expected service:

- `postgres` in `running` state

## 5. Build the React frontend

```powershell
Set-Location apps/web-lab
npm.cmd install
npm.cmd run build
Set-Location ../..
```

If you use Git Bash:

```bash
cd apps/web-lab
npm install
npm run build
cd ../..
```

## 6. Run the gateway

The gateway serves both the API and the frontend-lab.

```powershell
cargo run -p gateway
```

Expected log:

```text
gateway listening on 127.0.0.1:3000
```

The first start also applies SQL migrations automatically.

## 7. Open the frontend

Open:

```text
http://127.0.0.1:3000
```

What you should see:

- Provider cards for Codex and Claude
- Usage panel
- Chat creation controls
- Chat timeline
- Run stream and auth stream panels

## 8. Authenticate providers from the UI

The frontend already includes provider login/logout actions.

Recommended flow:

1. Click `Refresh` in the Providers panel.
2. Click `Login` for Codex or Claude.
3. Watch the `Auth stream` panel for device-login output or status messages.
4. Refresh providers again if needed.

Notes:

- Codex local runs use `codex.cmd` on Windows.
- Claude may fail on Windows if Git Bash is missing.

## 9. Create a chat and send a prompt

1. Choose a provider in the left sidebar.
2. Click `New chat`.
3. Write a prompt in the composer.
4. Click `Run prompt`.

What happens:

- The gateway stores the user message in Postgres
- A provider subprocess is started
- Events stream back over SSE
- Usage and raw events are persisted
- The assistant response is appended to the chat

## 10. Verify local health

### Browser checks

- `Providers` panel loads
- `Usage` panel loads
- New chats appear in the sidebar
- `Run stream` updates during execution

### API checks

```powershell
Invoke-RestMethod http://127.0.0.1:3000/api/health
Invoke-RestMethod http://127.0.0.1:3000/api/providers
Invoke-RestMethod http://127.0.0.1:3000/api/chats
```

## 11. Stop the stack

Stop the Rust gateway with `Ctrl+C`.

Stop Postgres:

```powershell
docker compose down
```

If you want to remove the Postgres data volume too:

```powershell
docker compose down -v
```

## Troubleshooting

### Codex command fails in PowerShell

This project calls `codex.cmd` from Rust on Windows, which avoids the common `codex.ps1` execution-policy problem. If you manually test commands in PowerShell, prefer:

```powershell
codex.cmd --version
```

### Claude says Git Bash is required

Install Git for Windows, then either:

- ensure `C:\Program Files\Git\bin\bash.exe` is in place, or
- set `CLAUDE_CODE_GIT_BASH_PATH`

Example:

```powershell
$env:CLAUDE_CODE_GIT_BASH_PATH="C:\Program Files\Git\bin\bash.exe"
```

### The frontend opens but providers show errors

Check:

- the CLI is installed
- the CLI is authenticated
- Docker/Postgres is running
- the gateway terminal has no startup migration errors

### Port 3000 or 5432 is already in use

Change either:

- `BIND_ADDR` in `.env`
- the published port in `docker-compose.yml`

## Local deployment summary

For local deployment, the full startup path is:

1. Clone repo
2. Install Rust, Docker, Codex CLI, Claude Code
3. Copy `.env.example` to `.env`
4. Run `docker compose up -d`
5. Build frontend (`npm run build` in `apps/web-lab`)
6. Run `cargo run -p gateway`
7. Open `http://127.0.0.1:3000`
