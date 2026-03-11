# Providers vNext: Secure Local Model Marketplace

This document is the context anchor for the next iteration on Ferrum AI
provider management and local model operations. It captures the agreed product
direction, implementation constraints, and the execution-ready plan so future
iterations do not depend on chat history.

## Goal

Turn `Providers` into the serious control plane for:

- closed-provider auth and configuration,
- local model inventory,
- secure local model installation,
- and hardware-aware recommendations for local runtimes.

`MCPs` must remain focused on MCP infrastructure, not model management.

## Locked product decisions

### Providers structure

`Providers` will contain three internal sections:

- `Closed Providers`
- `Local Models`
- `Local Model Marketplace`

The marketplace should further expose:

- `Installed`
- `Recommended`
- `Catalog`
- `Import GGUF`

### Security defaults

Ferrum must be secure by default and secure by design:

- no arbitrary model-name input for Ollama installs,
- no arbitrary URL input for GGUF downloads,
- no unrestricted internet download flow in v1,
- only curated catalog entries may be installed,
- GGUF downloads require checksum verification,
- install destinations must be controlled by Ferrum,
- and the default operating mode is enterprise-restricted.

Policy states shown in UI:

- `Approved for install`
- `Visible but blocked by policy`
- `Already installed`

### Runtime-specific install policy

#### Ollama

- real install flow is allowed in v1,
- but only for models approved in the curated catalog,
- executed by the Ferrum backend,
- with progress and audit.

#### llama.cpp / GGUF

Ferrum must support both:

- importing/registering an already-downloaded GGUF,
- managed download of approved GGUF files from the curated catalog.

GGUF managed download must use:

- approved origin from the catalog,
- checksum verification,
- controlled destination path,
- automatic registration after verification.

### Hardware authority

Ferrum must detect both:

- browser-side hardware snapshot,
- host-side hardware profile.

But the authoritative source for compatibility and installation is:

- the **Ferrum host**.

Browser hardware is UX context only and must be shown separately in the UI.

### Catalog and benchmarks

The curated catalog source for v1 is:

- an embedded JSON or local backend registry in the repo.

Remote benchmark enrichment is:

- optional,
- backend-only,
- cached,
- and never required for the marketplace to work.

Sources like `Artificial Analysis` may enrich model metadata, but Ferrum must
remain functional without them.

External benchmark or scraped data must **not** be stored in RAG in v1. Keep it
in structured cache or registry form.

### Identity and audit

Ferrum does not need full multi-user auth in this phase.

Instead, v1 must support logical attribution fields such as:

- `actor_name`
- `source_app`
- `source_channel`

This is required so future API, CLI, MCP, and other SaaS consumers can attribute
actions without redesigning the model.

### Goals taxonomy for recommendations

Marketplace objectives for v1:

- `chat`
- `coding`
- `reasoning`
- `analysis`
- `vision`
- `document_extraction`

More advanced recommendation categories like `agent_orchestration`,
`low_latency`, or `multimodal_enterprise` are deferred.

### Scope boundaries for v1

Included:

- structured `Providers` refactor,
- hardware detection and display,
- curated local model marketplace,
- secure Ollama install flow,
- GGUF import and managed install,
- audit model for installs and usage attribution,
- recommendation scoring based on hardware fit and objective.

Not included:

- full auth / RBAC,
- arbitrary remote download sources,
- closed-provider comparison inside the local marketplace,
- benchmark data in RAG,
- advanced skill-affinity-based recommendations.

## Execution-ready implementation plan

### 1. Frontend structure

Refactor `Providers` into three clear subsections:

- `Closed Providers`
- `Local Models`
- `Local Model Marketplace`

The new `Local Model Marketplace` should surface:

- installed local models,
- recommended models by objective and hardware fit,
- full curated catalog,
- controlled GGUF import.

UX priority is clarity and operational trust, not visual flourish.

### 2. Backend domain

Add a backend domain for local model operations and recommendations.

Minimum new capabilities:

- host hardware profile endpoint,
- browser hardware snapshot endpoint,
- curated catalog endpoint,
- installed model inventory endpoint,
- Ollama install job endpoint,
- install progress endpoint,
- GGUF import endpoint,
- GGUF managed install endpoint,
- usage attribution persistence.

Suggested endpoint surface:

- `GET /api/providers/hardware`
- `POST /api/providers/hardware/browser-snapshot`
- `GET /api/local-models/catalog`
- `GET /api/local-models/installed`
- `POST /api/local-models/ollama/install`
- `GET /api/local-models/install-jobs`
- `GET /api/local-models/install-jobs/{job_id}/stream`
- `POST /api/local-models/gguf/import`
- `POST /api/local-models/gguf/install`
- `POST /api/local-models/gguf/{id}/enable`

### 3. Data model

Introduce structured entities for:

- curated catalog entries,
- install jobs,
- hardware profiles,
- usage attribution.

The data model must be structured, auditable, and policy-aware rather than
free-form.

### 4. Recommendation engine

Implement a Ferrum-owned scoring layer inspired by `canirun.ai`, but based on
host authority.

Recommendation buckets:

- `Recommended`
- `Possible with tradeoffs`
- `Visible but blocked`
- `Not recommended`

Scoring inputs:

- host RAM and hardware profile,
- browser context snapshot,
- model size and quantization,
- context window,
- objective tags,
- optional cached benchmark enrichment,
- policy fit.

### 5. Installation execution

Install actions must be executed by the Ferrum backend.

#### Ollama

- use the local Ollama API,
- allow install only from approved catalog entries,
- stream progress,
- register audit trail.

#### GGUF

- import existing local file safely, or
- download approved artifact from curated catalog,
- verify checksum,
- write into controlled model directory,
- register model for `llama.cpp`.

### 6. Audit and observability

Ferrum should persist:

- who initiated the install,
- model identity,
- expected and actual checksum,
- approved source,
- runtime target,
- timestamp,
- result,
- final path or registered runtime identity.

If it does not introduce significant complexity, Ferrum should also track model
usage attribution by actor:

- actor metadata,
- provider,
- model,
- tokens or usage totals,
- timestamp,
- optional chat or workflow link.

## Test and acceptance anchors

### Backend

- host hardware works without browser participation,
- browser snapshot can be attached separately,
- catalog loads without internet,
- blocked entries are visible but not installable,
- Ollama install rejects non-catalog models,
- GGUF managed install rejects checksum mismatch,
- GGUF import registers valid local files,
- install jobs persist actor attribution and result,
- optional remote benchmark enrichment failure does not break the catalog.

### Frontend

- `Providers` clearly separates the three subsections,
- UI distinguishes `Host` from `Browser`,
- local marketplace shows policy state clearly,
- Ollama install progress is visible,
- GGUF import path is exposed as controlled advanced flow,
- installed inventory reflects both Ollama and GGUF states.

### Product acceptance

- operator can see what the host can realistically run,
- operator can install an approved Ollama model centrally,
- operator can import an approved GGUF local file,
- operator cannot trigger arbitrary model downloads,
- Ferrum still works in offline or restricted enterprise mode,
- future API/CLI/MCP consumers can attribute actions through logical actor
  fields.

## Implementation handoff

When starting the next implementation iteration, assume:

- `Providers` is the single home for local model operations,
- backend is the install executor,
- host is the authority for compatibility,
- catalog is embedded locally first,
- enterprise-restricted mode is the default,
- and external benchmark sources are optional enrichers only.

This file should be kept aligned with:

- `AGENT_PLATFORM_CONTEXT.md`
- `docs/architecture/skills-library-requirements.md`

If the marketplace scope, hardware authority model, or security posture changes,
update this document first before implementation expands.
