# Local Startup

Short startup path for Ferrum on `Windows 11` and `Arch Linux`.

## 1. Start PostgreSQL

```bash
docker compose up -d
```

Check:

```bash
docker compose ps
```

## 2. Build the frontend

### Windows 11

```powershell
Set-Location apps/web-lab
npm.cmd ci
npm.cmd run build
Set-Location ../..
```

### Arch Linux

```bash
cd apps/web-lab
npm ci
npm run build
cd ../..
```

## 3. Run the gateway

```bash
cargo run -p gateway
```

Open:

```text
http://127.0.0.1:3000
```

## 4. Quick verification

- `Providers` loads
- `Skills` loads
- `Local Model Marketplace` loads
- `Governance` shows host authority
