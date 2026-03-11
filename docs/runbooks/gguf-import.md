# GGUF Import

Use this for the controlled advanced path in `Providers`.

## 1. Prepare the file

Ferrum only accepts:

- an existing local `.gguf` file
- absolute path, or
- path relative to `LLAMA_CPP_MODEL_DIR`

## 2. Import from the UI

Go to:

```text
Providers > Local Model Marketplace > Import GGUF
```

Fill:

- alias
- file path
- optional quantization
- optional context window

## 3. Expected result

Ferrum will:

- validate `.gguf`
- verify the file exists
- compute SHA-256
- register it in the managed `llama.cpp` inventory
- store an install/audit job

## 4. Verify

Check:

- `Providers > Local Models`
- `Providers > Governance`

The model should appear in inventory and a completed job should exist.
