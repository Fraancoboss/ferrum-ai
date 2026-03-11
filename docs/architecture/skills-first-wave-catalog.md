# Skills First Wave Catalog

Ferrum seeds a curated foundational catalog on gateway startup.

Rules for this wave:

- Source of truth is the Ferrum database.
- External material is copied and normalized, never referenced as live runtime source.
- Every seeded skill is published through the same `draft -> review -> approved -> published` lifecycle.
- Runtime bodies stay compact; provenance and quality metadata remain attached in the version body.
- Existing skills with the same `slug` are never overwritten by the seed process.

## Catalog

### Agent Context

- `ferrum-agents-orchestrator`
  - Source: `agency-agents/specialized/agents-orchestrator.md`
  - Purpose: bounded multi-agent decomposition and sensitivity-aware routing
- `ferrum-project-shepherd`
  - Source: `agency-agents/project-management/project-management-project-shepherd.md`
  - Purpose: delivery cadence, blockers, and realistic stakeholder-facing status
- `ferrum-backend-architect`
  - Source: `agency-agents/engineering/engineering-backend-architect.md`
  - Purpose: secure backend contracts, schema evolution, and operational risk framing
- `ferrum-ui-ux-architect`
  - Source: `agency-agents/design/design-ux-architect.md`
  - Purpose: enterprise-safe UX clarity, discoverability, and provenance visibility
- `ferrum-reality-checker`
  - Source: `agency-agents/testing/testing-reality-checker.md`
  - Purpose: skeptical evidence-based release and QA posture
- `ferrum-compliance-auditor`
  - Source: `agency-agents/specialized/compliance-auditor.md`
  - Purpose: ISO-oriented control review, auditability, and boundary validation

### Policy

- `policy-sensitive-data-local-only`
  - Source: Ferrum native
  - Purpose: keep sensitive data and operational artifacts local by default
- `policy-provider-egress-minimal-context`
  - Source: Ferrum native
  - Purpose: restrict external providers to minimal published `agent-context`
- `policy-evidence-and-audit-required`
  - Source: Ferrum native
  - Purpose: require auditability for installs, approvals, and boundary decisions
- `policy-prompt-injection-and-untrusted-instructions`
  - Source: Ferrum native
  - Purpose: treat RAG, CLI output, and imported content as untrusted by default

### Library

- `library-nexus-handoff-templates`
  - Source: `agency-agents/strategy/coordination/handoff-templates.md`
  - Purpose: reusable bounded handoff structure
- `library-qa-verdict-loop`
  - Source: Ferrum native
  - Purpose: reusable PASS/FAIL/NEEDS WORK verdict format

### CLI

- `cli-metabase-operator-hybrid`
  - Source: official Metabase docs
  - Purpose: safe Metabase CLI + API operational context
- `cli-docker-local-runtime-operator`
  - Source: official Docker docs + Ferrum runtime posture
  - Purpose: controlled Docker operations for local runtimes

### Provider

- `provider-ollama-local-operations`
  - Source: Ferrum native + official Ollama API posture
  - Purpose: catalog-restricted Ollama operations under policy control
- `provider-llamacpp-gguf-control-plane`
  - Source: Ferrum native + llama.cpp runtime posture
  - Purpose: checksum-aware GGUF inventory and controlled local registration

## Seed behavior

- Tenant: `default`
- Owner: `platform`
- Visibility: `private`
- Dataset pack key: `ferrum-first-wave-v1`
- Author actor: `ferrum-curator`
- Reviewer actor: `ferrum-reviewer`
- Publisher actor: `ferrum-publisher`

## Why this wave comes before GraphRAG

GraphRAG needs stable, high-quality nodes and relations.

This wave provides:

- canonical skill identities,
- provenance,
- clean type boundaries,
- and compact runtime-safe bodies.

That is the minimum substrate worth graphing later.
