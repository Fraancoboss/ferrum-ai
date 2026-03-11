# Agent platform context

This file captures the current product direction for Ferrum AI around
**Agents**, **Skills**, **MCPs**, and future CLI-driven operations. It gives
future agents and contributors one stable source of product intent before they
propose or implement changes.

## Why this file exists

`CONTEXT.md` explains the original MVP: a local browser UI that orchestrates
provider CLIs, persists chats and runs, and prepares the database for later RAG
work. This file captures the next layer of the product without mixing roadmap
intent into the MVP origin story.

## Product direction

Ferrum AI is moving from a local orchestration lab into a more complete
operator console for AI workflows. The product direction is broader than
running prompts. Ferrum AI must also manage:

- multi-agent workflows,
- reusable skills,
- provider-agnostic MCP connections,
- future CLI automation surfaces,
- and local model operations.

The product must stay comfortable for business users while remaining precise
enough for technical operators and compliance-heavy environments.

## External inspiration

Ferrum AI uses the project at
`C:\Users\fraan\proyectos\agency-agents` as a design reference. The important
idea is the agent specialization model, especially the specialist and
role-driven structure. Ferrum AI must adapt that pattern to its own runtime,
data boundaries, and governance model instead of copying it literally.

In practice, Ferrum AI should let users:

- discover many more agent types,
- understand what each agent is good at,
- map agents to business goals and workflows,
- and compose agents safely under different sensitivity levels.

## Skills are a first-class domain

Ferrum AI treats **Skills** as a first-class product domain. A skill is a
versioned, reusable unit of expert context, operating guidance, or tool usage
knowledge that can be assigned to agents, providers, workflows, or future CLI
automation surfaces.

The product uses one unified **Skills** library. Ferrum AI does not create a
separate sidebar section for CLI-specific skills. Instead, the library uses
formal skill types and strong filtering.

The current canonical skill types are:

- `library`
- `agent-context`
- `cli`
- `provider`
- `policy`

This means the expert context that shapes an orchestrator or specialist agent
also lives in the same library as operational and CLI-facing skills.

## Agent context and external providers

Ferrum AI separates reusable local skills from the minimum context sent to
external providers such as Codex or Claude. External providers may participate
in a workflow, but they must not receive the full library by default.

The operating rule is:

- local agents consume skills from the shared library,
- external providers receive only the minimum `agent-context` needed for the
  assigned role,
- and sensitive workflow artifacts stay local unless policy explicitly permits
  a narrower release.

For `internal` and `sensitive` workflows, external providers do not receive
general library skills by default. They receive only the minimum approved
`agent-context` needed for their role.

This separation exists to support ISO 27001 and ISO 42001 style controls around
data minimization, traceability, and bounded exposure.

## Canonical storage and RAG

Ferrum AI uses a structured registry as the source of truth for skills. The
database already includes `documents`, `chunks`, and `pgvector` tables that can
support retrieval use cases. That retrieval layer is useful, but it is not the
canonical source of skill state.

The storage rule is:

- structured skill records are canonical,
- RAG is derived from the canonical records,
- search and retrieval may use embeddings,
- but approval state, ownership, sensitivity, assignments, and versioning must
  live in the structured registry.

The retrieval layer indexes approved or published skill content only. Draft and
review-stage skills remain outside the general semantic index by default.

Ferrum AI may later add an export and import layer for curated datasets, seed
packs, or team-to-team portability, but those exports are derivatives of the
canonical database state, not a second source of truth.

This lets Ferrum AI use semantic retrieval without losing governance or
deterministic control.

## CLI-Skills

Ferrum AI treats **CLI-Skills** as `cli` skills inside the unified library. A
CLI-Skill teaches an agent how to operate a specific CLI safely and
consistently.

The standard lifecycle is:

1. ingest CLI context from docs, help output, examples, and local source,
2. extract structure such as commands, flags, outputs, and failure modes,
3. generate a draft `cli` skill,
4. require human review and approval,
5. publish a versioned skill,
6. and index the approved version into the derived retrieval layer.

CLI-Skills are useful when you want an agent to use a CLI as if it had a
tool-like integration. They do not replace MCP in every case, but they create
an important middle layer between free-form prompting and full tool protocol
integration.

CLI-Skills remain contextual and descriptive. They do not execute commands by
themselves. They teach an agent how to reason about a CLI and how to prepare
safe execution through another runtime surface.

## Sensitivity model

Ferrum AI formalizes three sensitivity levels:

- `public`
- `internal`
- `sensitive`

The product direction is **public is flexible, non-public is local-first**.
The current code already approximates this by gating external providers for
non-public workflows. Ferrum AI now needs an explicit matrix that defines what
each level permits for orchestrators, subagents, skills, providers, and
artifacts.

The baseline policy is:

- `public`: external providers and local agents may participate more freely,
- `internal`: local-first; external providers can assist, but only with minimum
  role context,
- `sensitive`: sensitive data stays with local agents by default; any external
  participation must remain tightly minimized and auditable.

## Current focus

The near-term work is to make the Skills domain real in both documentation and
architecture. That includes the library model, the sensitivity matrix, the
relationship between local skills and external providers, and the future path
for CLI-Skills.

This is not only a UI project. It is a product architecture and governance
project.

## Governance model

Ferrum AI must keep the governance model ready for multi-tenant and
microservice-style reuse, even if the early product remains single-user.

The logical roles are:

- `author`
- `reviewer`
- `publisher`

One operator may temporarily perform all three roles in the lab product, but
the canonical schema and audit trail must preserve the distinction from the
start.

## Required UX direction

The future Skills experience must let users work with a large library without
turning the product into a cluttered admin console.

The UX must support:

- browsing one shared library,
- filtering by `skill_type`, sensitivity, tags, owner, and status,
- understanding which skills are safe for which workflows,
- distinguishing `agent-context` from operational library skills,
- and finding `cli` skills quickly without a separate navigation domain.

The runtime selection model is hybrid:

- administrators assign skills explicitly to templates, roles, or providers,
- the system resolves the allowed skill set from those assignments and policy,
- and the orchestrator chooses only within that governed subset.

## Related roadmap items

The following roadmap items remain active and must stay compatible with this
direction:

- a dedicated **MCPs** section for shared MCP infrastructure,
- future CRUD for Skills,
- visibility into an agent's assigned `agent-context` and related skills,
- a future Ferrum CLI,
- a `ferrum-cli-use` skill for AI-driven CLI operation,
- an MCP surface for the Ferrum CLI,
- and backend support for Ollama and `llama.cpp` model operations and hardware
  compatibility reporting.

## Documentation anchor

The detailed requirement spec for this domain lives in:

- `docs/architecture/skills-library-requirements.md`

That file defines the implementation-ready requirements for the Skills
library, the CLI-Skill lifecycle, the sensitivity matrix, and the RAG-derived
indexing model.

## Next steps

Use this file as the product summary and use
`docs/architecture/skills-library-requirements.md` as the detailed
implementation reference. When you change the Skills, Agents, MCPs, or
sensitivity model, update both documents together so product intent and system
requirements stay aligned.
