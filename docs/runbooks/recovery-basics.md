# Recovery Basics

Use this when local providers or model operations look broken.

## 1. PostgreSQL

```bash
docker compose ps
docker compose up -d
```

## 2. Gateway

Restart:

```bash
cargo run -p gateway
```

If startup fails, fix that first before debugging the UI.

## 3. Ollama host runtime

Check:

```bash
ollama --version
ollama list
```

If this fails, Ferrum will also fail to query local Ollama inventory.

## 4. Governance view

Open:

```text
Providers > Governance
```

Use it to confirm:

- current endpoint
- runtime mode
- recent job failures
- inventory issues

## 5. GGUF path issues

If a GGUF import fails:

- confirm the file exists
- confirm it ends with `.gguf`
- confirm the path is absolute or relative to `LLAMA_CPP_MODEL_DIR`
