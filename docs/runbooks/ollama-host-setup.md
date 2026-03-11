# Ollama Host Setup

Ferrum is `host-first` for Ollama. Docker support stays optional.

## 1. Install Ollama on the host

Follow the official installer for your OS:

- Windows 11: install the desktop/runtime package
- Arch Linux: use your preferred package path or the official install script

## 2. Verify the daemon

```bash
ollama --version
ollama list
```

Expected:

- the command exists
- the daemon answers on the host

## 3. Confirm the endpoint used by Ferrum

Default endpoint:

```text
http://127.0.0.1:11434
```

If needed, set:

```dotenv
OLLAMA_API_BASE=http://127.0.0.1:11434
```

## 4. Validate from Ferrum

Open `Providers > Governance` and check:

- `Ollama mode: Host`
- endpoint visible
- no runtime connectivity issue

Then open `Providers > Local Models` and confirm inventory is visible.
