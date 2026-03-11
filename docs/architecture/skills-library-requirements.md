# Skills library requirements

This document defines the implementation-ready requirements for the Ferrum AI
Skills library. It explains what the system must do, how it must relate to
agents and providers, how it must fit the current codebase, and how it must
support ISO 27001 and ISO 42001 style controls.

## Current baseline

Ferrum AI already has several pieces that shape this design. The backend has a
workflow model with `sensitivity`, workflow artifacts, approvals, evidence
records, snapshots, MCP registry data, and `llama.cpp` model records. The
database also already contains `documents`, `chunks`, and `pgvector`-backed
embeddings.

The current workflow runtime uses a simple sensitivity rule:

- `public` workflows allow external provider participation more freely,
- non-public workflows gate external providers,
- and local agents remain the default operators for evidence and release-style
  responsibilities.

This document turns that baseline into an explicit Skills architecture instead
of leaving the behavior implicit.

## Summary of decisions

Ferrum AI uses one shared **Skills** library with formal skill types. The
canonical store is structured and versioned. The retrieval layer is derived
from the canonical store. CLI-Skills are a skill type, not a separate product
section. External providers receive only the minimum `agent-context` needed for
their role. Local agents consume library skills based on workflow policy and
sensitivity. Governance uses logical `author`, `reviewer`, and `publisher`
roles. Skill resolution is hybrid: explicit assignment plus governed runtime
selection within the allowed subset.

## Product model

The Skills library exists to give Ferrum AI one governed system for reusable
expert context and operational knowledge.

The library must support these use cases:

- define expert context for orchestrators and specialist agents,
- attach reusable operating knowledge to local agents,
- teach an agent how to operate a specific CLI,
- model provider-specific runtime guidance,
- and capture policy or governance guidance that constrains execution.

The library is not a free-form prompt dump. Every skill must carry enough
structure to support filtering, approval, assignment, and auditability.

## Skill taxonomy

Ferrum AI must support the following formal `skill_type` values.

### `library`

Use this type for reusable operational expertise that is not tied to a single
agent identity, provider, or CLI.

Examples:

- a data warehouse analysis skill,
- a testing skill,
- a support triage skill.

### `agent-context`

Use this type for reusable expert context that defines how an agent behaves or
what perspective it operates from.

Examples:

- orchestrator context,
- planner context,
- specialist context imported or adapted from `agency-agents`.

### `cli`

Use this type for CLI-Skills. A `cli` skill teaches an agent how to operate a
specific CLI safely and consistently.

Examples:

- `ferrum-cli-use`,
- a deployment CLI skill,
- a data pipeline CLI skill.

### `provider`

Use this type for provider-specific guidance that is coupled to one runtime,
model family, or execution environment.

Examples:

- Codex-specific prompt handling guidance,
- Ollama runtime tuning context,
- `llama.cpp` execution constraints.

### `policy`

Use this type for control-oriented skills that encode governance, compliance,
or safety rules.

Examples:

- ISO-aligned handling policy,
- data minimization rules,
- approval requirements for high-risk tasks.

## Canonical model and derived retrieval

Ferrum AI must use a structured registry as the source of truth for the Skills
domain. The retrieval layer must be derived from that registry.

This rule exists because Ferrum AI needs both retrieval quality and strong
governance. A pure document store or pure RAG design cannot reliably express:

- version history,
- ownership,
- approval state,
- assignment relationships,
- exposure policy,
- or sensitivity constraints.

The canonical store must own:

- skill identity,
- lifecycle state,
- versioning,
- owners and approvers,
- allowed sensitivity levels,
- provider exposure rules,
- and assignment state.

The RAG-derived layer may index:

- skill body content,
- examples,
- imported documentation,
- CLI help output,
- source snippets,
- and normalized summaries.

The RAG-derived layer must not become the source of truth for:

- publication state,
- access policy,
- approvals,
- assignments,
- or runtime enforcement.

The default indexing rule is:

- only `approved` or `published` skill versions are indexed into the general
  retrieval layer,
- `draft` and `review` versions remain outside the shared semantic index unless
  a later admin-only review index is introduced.

Ferrum AI may later support export and import flows for skill datasets, seed
packs, and team portability. Those artifacts are derived from canonical
records, not an alternate source of truth.

## Required entities

The implementation must introduce a structured model that can express Skills as
versioned, assignable, and governable objects.

At minimum, the design must include these entities.

### `Skill`

This is the stable identity of a skill across versions.

Minimum fields:

- `id`
- `slug`
- `name`
- `skill_type`
- `description`
- `status`
- `owner`
- `visibility`
- `tags`
- `allowed_sensitivity_levels`
- `provider_exposure`
- `source_kind`
- `created_at`
- `updated_at`

### `SkillVersion`

This stores the versioned content and structured body of a skill.

Minimum fields:

- `id`
- `skill_id`
- `version`
- `status`
- `body`
- `summary`
- `examples`
- `constraints`
- `review_notes`
- `created_by`
- `approved_by`
- `created_at`
- `approved_at`

### `SkillAssignment`

This links a published skill version to a runtime consumer.

Assignment targets must support at least:

- agent,
- workflow template,
- provider,
- and future CLI profile.

Minimum fields:

- `id`
- `skill_version_id`
- `target_type`
- `target_id`
- `assignment_mode`
- `created_at`

### `SkillSource`

This records how a skill or version was derived.

Source kinds must support at least:

- manual,
- imported_docs,
- cli_help,
- repository_scan,
- generated_from_template.

### `SkillApproval`

This records review and publication decisions. Ferrum AI needs this for audit,
especially for `cli`, `policy`, and `agent-context` skills.

The approval model must distinguish the logical roles of:

- `author`
- `reviewer`
- `publisher`

The first release may let one operator hold all three roles, but the schema and
audit model must preserve the separation.

### `SkillIndexDocument`

This links canonical skill content to the derived retrieval layer so the system
can reindex deterministically.

### `SensitivityPolicy`

This stores or encodes the routing matrix that maps a workflow sensitivity to
allowed agent, provider, and skill behavior.

## Skill lifecycle

Each skill version must move through an explicit lifecycle. The first version
of the lifecycle must support these states:

- `draft`
- `review`
- `approved`
- `published`
- `retired`

Only approved or published versions may be assigned to runtime consumers by
default. A later admin override model can relax this for development or lab
work, but that exception must be explicit and auditable.

## CLI-Skill workflow

Ferrum AI must standardize CLI-Skill creation so it can be repeated safely for
Ferrum's own CLI and for external applications.

The default CLI-Skill workflow is:

1. ingest CLI context,
2. extract structure,
3. generate a draft skill,
4. review and approve,
5. publish a version,
6. index the published version into retrieval,
7. assign the published skill where needed.

### Ingest CLI context

Ferrum AI must support reading CLI context from:

- `README` files,
- local docs,
- `--help` output,
- subcommand help,
- example invocations,
- and repository source when available.

### Extract structure

The extracted representation must capture at least:

- commands,
- subcommands,
- arguments,
- flags,
- environment dependencies,
- expected outputs,
- failure modes,
- side effects,
- and security constraints.

### Generate a draft skill

The system may use AI assistance to generate the first draft, but the draft
must become a structured `cli` skill version.

### Review and approve

CLI-Skills must require human review before publication. This matters because
they can drive external systems and create tool-like behavior without an MCP.

### Publish and assign

Once approved, the CLI-Skill becomes assignable to agents, templates, or future
CLI automation surfaces such as `ferrum-cli-use`.

CLI-Skills remain descriptive and contextual. They do not execute commands on
their own. They teach an agent how to understand a CLI and how to prepare
correct execution through another approved runtime surface.

## Agent-context and `agency-agents`

Ferrum AI must treat reusable agent persona and role context as first-class
`agent-context` skills inside the same library.

This is how Ferrum AI can reuse the value of
`C:\Users\fraan\proyectos\agency-agents` without copying that repository into
runtime behavior verbatim.

The import or adaptation model must support:

- taking a specialist context from `agency-agents`,
- normalizing it into structured Ferrum fields,
- versioning it as `agent-context`,
- and assigning it to one or more Ferrum agents or workflow templates.

This keeps agent identity reusable and governable instead of hardcoding long
prompt blocks into one workflow path.

## Sensitivity matrix

Ferrum AI must formalize the routing and exposure rules for:

- `public`
- `internal`
- `sensitive`

The following matrix is the first required operating policy.

| Sensitivity | Orchestrator | Subagents | Library skill usage | External provider exposure | Approval expectation |
| --- | --- | --- | --- | --- | --- |
| `public` | local or external | local or external | allowed according to assignment | minimum `agent-context`; broader exposure may be allowed by policy | optional, based on action risk |
| `internal` | local preferred, external allowed with minimization | local preferred; external only when justified | local agents may use assigned library skills | only minimum `agent-context`; never full library or internal artifacts by default | required for external participation on non-trivial actions |
| `sensitive` | local preferred and usually required | local by default | local agents use approved skills only | only minimum abstracted `agent-context`; no sensitive artifacts or broad library release | strong approval and full evidence trail |

The current code already gates non-public external execution. The new system
must preserve that behavior while making the policy explicit and enforceable at
the skill and assignment layer.

## Separation of local and external execution context

Ferrum AI must keep these concepts separate:

- `agent-context` sent to an external provider,
- library skills available to local agents,
- workflow artifacts,
- sensitive data,
- and policy constraints.

This means:

- external providers do not receive the full library by default,
- local agents can hold richer skill state,
- and the system must be able to explain which context was exposed to whom.

For `internal` and `sensitive` workflows, the baseline enforcement rule is:

- external providers do not receive general library skills,
- external providers receive only the minimum approved `agent-context` needed
  for their assigned role,
- local agents consume the richer assigned skill set inside the governed
  runtime boundary.

## Provider exposure policy

Each skill must carry a `provider_exposure` policy. The first version of the
policy model must support at least:

- `local_only`
- `agent_context_only`
- `provider_allowed`

The default by type is:

- `library`: `local_only`
- `agent-context`: `agent_context_only`
- `cli`: `local_only`
- `provider`: `provider_allowed`
- `policy`: `local_only`

An implementation may later add finer controls, but these defaults must anchor
the first release.

## Required API and UI capabilities

Ferrum AI must expose enough surface area to make the Skills library operable.

The minimum future API/UI contract must support:

- list skills,
- filter by `skill_type`, tags, owner, status, and sensitivity,
- view a skill detail page,
- view version history,
- create a skill,
- create a CLI-Skill draft from ingested CLI context,
- submit a version for review,
- approve and publish a version,
- assign a skill to an agent, provider, or workflow template,
- and trigger reindexing into the derived retrieval layer.

The UI must keep one shared Skills library and add:

- strong type filters,
- saved views,
- ordering by update date, usage, and risk,
- and a clear difference between `agent-context`, `cli`, and general library
  skills.

The assignment and resolution model must be hybrid:

- administrators assign skills explicitly to workflow templates, agent roles,
  providers, or future CLI profiles,
- tags and metadata support discovery and filtering,
- the runtime resolves the allowed skill set from assignments plus policy,
- and the orchestrator chooses only within that governed subset.

## Data and audit requirements

Ferrum AI already has evidence and audit-oriented workflow tables. The Skills
domain must integrate with that posture.

The implementation must make it possible to answer:

- who created a skill version,
- what source material it came from,
- who approved it,
- which consumers use it,
- which sensitivity levels it is allowed in,
- and whether any external provider ever received derived context from it.

## Relationship with current workflow runtime

The current runtime defines default agent roles such as planner, researcher,
coder, evidence collector, and reality checker. It also selects providers for
those roles and uses sensitivity to gate some external execution.

The Skills system must extend that runtime instead of replacing it. The first
implementation path must support:

- attaching `agent-context` to those role templates,
- attaching local library skills to local roles,
- and enforcing sensitivity-based exposure rules when external providers are
  used for researcher or coder style roles.

This means the orchestrator does not select from the whole library freely. It
selects from the subset made eligible by:

- workflow template assignment,
- agent role assignment,
- provider assignment,
- sensitivity policy,
- and provider exposure rules.

## Documentation and implementation phases

Ferrum AI should deliver this domain in phases so governance stays ahead of
automation.

### Phase 1: canonical model and docs

Deliver:

- final documentation,
- canonical skill entities,
- versioning,
- basic listing and filtering,
- and the sensitivity matrix.

### Phase 2: CLI-Skill ingestion

Deliver:

- ingest sources,
- draft generation,
- review flow,
- and publication.

### Phase 3: RAG-derived indexing

Deliver:

- deterministic indexing from published skill versions,
- retrieval support,
- and reindex operations.

### Phase 4: runtime assignment

Deliver:

- assignment to agents, providers, and templates,
- enforcement of provider exposure rules,
- and visibility in the workflow UI.

## Acceptance scenarios

Ferrum AI must satisfy these scenarios once the feature is implemented:

1. You create a `library` skill and assign it to a local agent.
2. You import or adapt an `agency-agents` specialist into an `agent-context`
   skill.
3. You ingest a third-party CLI and publish a reviewed `cli` skill.
4. You run a `public` workflow that uses an external provider with only the
   intended minimum context.
5. You run an `internal` workflow where an external orchestrator participates,
   but library skills remain local.
6. You run a `sensitive` workflow where sensitive artifacts stay local and the
   evidence trail explains every exposure decision.
7. You filter the Skills library by `cli`, `agent-context`, and `policy`.
8. You prove who created, approved, and assigned a published skill.
9. You prove that a `draft` or `review` skill version was not exposed through
   the shared semantic retrieval layer.
10. You prove that an `internal` or `sensitive` workflow exposed only minimum
    approved `agent-context` to an external provider.

## Next steps

Use this document as the implementation reference for the Skills library and
update it whenever the product changes the skill taxonomy, the sensitivity
matrix, the canonical data model, or the RAG-derived indexing strategy. Keep
this file aligned with `AGENT_PLATFORM_CONTEXT.md` so the product summary and
the implementation spec never drift apart.
