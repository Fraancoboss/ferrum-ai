import { useEffect, useMemo, useState } from "react";
import type { FormEvent } from "react";

import { api } from "./api";
import type {
  SkillAssignmentTargets,
  SkillDetail,
  SkillProviderExposure,
  SkillSummary,
  SkillType,
  SkillVersion,
} from "./types";

const SKILL_TYPES: SkillType[] = ["library", "agent-context", "cli", "provider", "policy"];
const SENSITIVITY_LEVELS = ["public", "internal", "sensitive"] as const;
const ACTOR_NAME = "local-operator";

type FiltersState = {
  skillType: string;
  status: string;
  tag: string;
  sensitivity: string;
};

type CreateFormState = {
  name: string;
  slug: string;
  skillType: SkillType;
  description: string;
  owner: string;
  tags: string;
  allowedSensitivity: string[];
  summary: string;
  body: string;
  examples: string;
  constraints: string;
  reviewNotes: string;
  sourceRef: string;
  datasetPackKey: string;
};

type VersionFormState = {
  summary: string;
  body: string;
  examples: string;
  constraints: string;
  reviewNotes: string;
  sourceRef: string;
  datasetPackKey: string;
};

type AssignmentFormState = {
  skillVersionId: string;
  targetType: "workflow_template" | "agent_role" | "provider";
  targetKey: string;
};

const initialCreateForm = (): CreateFormState => ({
  name: "",
  slug: "",
  skillType: "library",
  description: "",
  owner: "platform",
  tags: "",
  allowedSensitivity: ["internal"],
  summary: "",
  body: "",
  examples: "",
  constraints: "",
  reviewNotes: "",
  sourceRef: "",
  datasetPackKey: "",
});

const initialVersionForm = (): VersionFormState => ({
  summary: "",
  body: "",
  examples: "",
  constraints: "",
  reviewNotes: "",
  sourceRef: "",
  datasetPackKey: "",
});

const initialAssignmentForm = (): AssignmentFormState => ({
  skillVersionId: "",
  targetType: "agent_role",
  targetKey: "",
});

export function SkillsScreen() {
  const [filters, setFilters] = useState<FiltersState>({
    skillType: "",
    status: "",
    tag: "",
    sensitivity: "",
  });
  const [skills, setSkills] = useState<SkillSummary[]>([]);
  const [selectedSkillId, setSelectedSkillId] = useState<string | null>(null);
  const [selectedDetail, setSelectedDetail] = useState<SkillDetail | null>(null);
  const [createForm, setCreateForm] = useState<CreateFormState>(initialCreateForm);
  const [versionForm, setVersionForm] = useState<VersionFormState>(initialVersionForm);
  const [assignmentForm, setAssignmentForm] = useState<AssignmentFormState>(initialAssignmentForm);
  const [assignmentTargets, setAssignmentTargets] = useState<SkillAssignmentTargets | null>(null);
  const [listLoading, setListLoading] = useState(true);
  const [detailLoading, setDetailLoading] = useState(false);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const filteredStats = useMemo(() => {
    const cliCount = skills.filter((skill) => skill.skill_type === "cli").length;
    const contextCount = skills.filter((skill) => skill.skill_type === "agent-context").length;
    const publishedCount = skills.filter(
      (skill) => skill.latest_version_status === "published",
    ).length;

    return {
      total: skills.length,
      cliCount,
      contextCount,
      publishedCount,
    };
  }, [skills]);
  const publishedVersions = useMemo(
    () => selectedDetail?.versions.filter((version) => version.status === "published") ?? [],
    [selectedDetail],
  );

  async function loadSkills(nextSelectedSkillId?: string | null) {
    setListLoading(true);
    try {
      const items = await api.listSkills({
        skill_type: filters.skillType || undefined,
        status: filters.status || undefined,
        tag: filters.tag || undefined,
        sensitivity: filters.sensitivity || undefined,
      });
      setSkills(items);

      const preferredSkillId = nextSelectedSkillId ?? selectedSkillId;
      const selectedExists = preferredSkillId
        ? items.some((item) => item.id === preferredSkillId)
        : false;
      const fallbackSkillId = items[0]?.id ?? null;
      setSelectedSkillId(selectedExists ? preferredSkillId : fallbackSkillId);
    } catch (loadError) {
      setError(normalizeError(loadError));
    } finally {
      setListLoading(false);
    }
  }

  async function loadSkillDetail(skillId: string) {
    setDetailLoading(true);
    try {
      const detail = await api.getSkill(skillId);
      setSelectedDetail(detail);
      setVersionForm(initialVersionForm());
      setAssignmentForm((current) => ({
        ...current,
        skillVersionId:
          detail.versions.find((version) => version.status === "published")?.id ?? "",
      }));
    } catch (loadError) {
      setError(normalizeError(loadError));
    } finally {
      setDetailLoading(false);
    }
  }

  async function loadAssignmentTargets() {
    try {
      const targets = await api.skillAssignmentTargets();
      setAssignmentTargets(targets);
      setAssignmentForm((current) => ({
        ...current,
        targetKey:
          current.targetKey ||
          targets.agent_roles[0] ||
          targets.workflow_templates[0] ||
          String(targets.providers[0] ?? ""),
      }));
    } catch (loadError) {
      setError(normalizeError(loadError));
    }
  }

  useEffect(() => {
    void loadSkills();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filters.skillType, filters.status, filters.tag, filters.sensitivity]);

  useEffect(() => {
    void loadAssignmentTargets();
  }, []);

  useEffect(() => {
    if (!selectedSkillId) {
      setSelectedDetail(null);
      return;
    }
    void loadSkillDetail(selectedSkillId);
  }, [selectedSkillId]);

  async function handleCreateSkill(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusyAction("create-skill");
    setError(null);
    setNotice(null);

    const slug = (createForm.slug.trim() || slugify(createForm.name)).trim();
    if (!slug) {
      setBusyAction(null);
      setError("Skill slug is required.");
      return;
    }

    try {
      const detail = await api.createSkill({
        slug,
        name: createForm.name.trim(),
        skill_type: createForm.skillType,
        description: createForm.description.trim(),
        owner: createForm.owner.trim() || "platform",
        visibility: "private",
        tags: parseDelimitedList(createForm.tags),
        allowed_sensitivity_levels: createForm.allowedSensitivity,
        provider_exposure: defaultProviderExposure(createForm.skillType),
        source_kind: "manual",
        initial_version: {
          summary: createForm.summary.trim(),
          body: {
            content: createForm.body.trim(),
          },
          examples: parseDelimitedList(createForm.examples),
          constraints: parseDelimitedList(createForm.constraints),
          review_notes: normalizeOptionalText(createForm.reviewNotes),
          created_by: ACTOR_NAME,
          source_ref: normalizeOptionalText(createForm.sourceRef),
          dataset_pack_key: normalizeOptionalText(createForm.datasetPackKey),
        },
      });
      setCreateForm(initialCreateForm());
      setSelectedDetail(detail);
      setSelectedSkillId(detail.skill.id);
      await loadSkills(detail.skill.id);
      setNotice(`Skill ${detail.skill.name} created as draft v1.`);
    } catch (createError) {
      setError(normalizeError(createError));
    } finally {
      setBusyAction(null);
    }
  }

  async function handleCreateVersion(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selectedDetail) return;
    setBusyAction("create-version");
    setError(null);
    setNotice(null);

    try {
      const detail = await api.createSkillVersion(selectedDetail.skill.id, {
        summary: versionForm.summary.trim(),
        body: {
          content: versionForm.body.trim(),
        },
        examples: parseDelimitedList(versionForm.examples),
        constraints: parseDelimitedList(versionForm.constraints),
        review_notes: normalizeOptionalText(versionForm.reviewNotes),
        created_by: ACTOR_NAME,
        source_ref: normalizeOptionalText(versionForm.sourceRef),
        dataset_pack_key: normalizeOptionalText(versionForm.datasetPackKey),
      });
      setSelectedDetail(detail);
      setVersionForm(initialVersionForm());
      await loadSkills(detail.skill.id);
      setNotice(`Draft v${detail.versions[0]?.version ?? "?"} created.`);
    } catch (actionError) {
      setError(normalizeError(actionError));
    } finally {
      setBusyAction(null);
    }
  }

  async function handleCreateAssignment(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selectedDetail) return;
    if (!assignmentForm.skillVersionId || !assignmentForm.targetKey) {
      setError("Choose a published version and a valid assignment target.");
      return;
    }

    setBusyAction("create-assignment");
    setError(null);
    setNotice(null);
    try {
      const detail = await api.createSkillAssignment(selectedDetail.skill.id, {
        skill_version_id: assignmentForm.skillVersionId,
        target_type: assignmentForm.targetType,
        target_key: assignmentForm.targetKey,
      });
      setSelectedDetail(detail);
      await loadSkills(detail.skill.id);
      setNotice("Assignment created.");
    } catch (actionError) {
      setError(normalizeError(actionError));
    } finally {
      setBusyAction(null);
    }
  }

  async function handleDeleteAssignment(assignmentId: string) {
    setBusyAction(`delete-assignment-${assignmentId}`);
    setError(null);
    setNotice(null);
    try {
      const detail = await api.deleteSkillAssignment(assignmentId);
      setSelectedDetail(detail);
      await loadSkills(detail.skill.id);
      setNotice("Assignment removed.");
    } catch (actionError) {
      setError(normalizeError(actionError));
    } finally {
      setBusyAction(null);
    }
  }

  async function handleVersionAction(
    version: SkillVersion,
    action: "review" | "approve" | "publish",
  ) {
    setBusyAction(`${action}-${version.id}`);
    setError(null);
    setNotice(null);

    try {
      const detail =
        action === "review"
          ? await api.submitSkillVersionForReview(version.id, { actor: ACTOR_NAME })
          : action === "approve"
            ? await api.approveSkillVersion(version.id, { actor: ACTOR_NAME })
            : await api.publishSkillVersion(version.id, { actor: ACTOR_NAME });
      setSelectedDetail(detail);
      await loadSkills(detail.skill.id);
      setNotice(
        action === "review"
          ? `Version v${version.version} sent to review.`
          : action === "approve"
            ? `Version v${version.version} approved.`
            : `Version v${version.version} published.`,
      );
    } catch (actionError) {
      setError(normalizeError(actionError));
    } finally {
      setBusyAction(null);
    }
  }

  return (
    <section className="skills-screen">
      <section className="card skills-hero">
        <div>
          <span className="eyebrow">Skills Library</span>
          <h3>Governed expertise for agents, providers, and future CLI surfaces</h3>
          <p className="section-copy">
            Skills now live as a real platform registry. The canonical state stays in the
            database, versioning is explicit, and the UI exposes the lifecycle without
            mixing runtime decisions into the catalog.
          </p>
        </div>

        <div className="agents-hero-stats">
          <SkillMetric label="Visible skills" value={String(filteredStats.total)} />
          <SkillMetric label="Published" value={String(filteredStats.publishedCount)} />
          <SkillMetric label="Agent context" value={String(filteredStats.contextCount)} />
          <SkillMetric label="CLI skills" value={String(filteredStats.cliCount)} />
        </div>
      </section>

      {error ? <p className="agent-error">{error}</p> : null}
      {notice ? <p className="skills-notice">{notice}</p> : null}

      <section className="skills-workbench">
        <article className="card skills-sidebar-panel">
          <div className="section-head">
            <div>
              <span className="eyebrow">Create</span>
              <h4>New skill</h4>
            </div>
            <span className="badge">draft first</span>
          </div>

          <form className="skills-form-grid" onSubmit={handleCreateSkill}>
            <div className="form-row">
              <label htmlFor="skill-name">Name</label>
              <input
                id="skill-name"
                value={createForm.name}
                onChange={(event) =>
                  setCreateForm((current) => {
                    const nextName = event.target.value;
                    const currentSlug = current.slug.trim();
                    const derivedCurrentSlug = slugify(current.name);
                    return {
                      ...current,
                      name: nextName,
                      slug:
                        !currentSlug || currentSlug === derivedCurrentSlug
                          ? slugify(nextName)
                          : current.slug,
                    };
                  })
                }
                placeholder="Sensitive handling policy"
                required
              />
            </div>

            <div className="form-row">
              <label htmlFor="skill-slug">Slug</label>
              <input
                id="skill-slug"
                value={createForm.slug}
                onChange={(event) =>
                  setCreateForm((current) => ({ ...current, slug: event.target.value }))
                }
                placeholder="sensitive-handling-policy"
                required
              />
            </div>

            <div className="form-row">
              <label htmlFor="skill-type">Type</label>
              <select
                id="skill-type"
                value={createForm.skillType}
                onChange={(event) =>
                  setCreateForm((current) => ({
                    ...current,
                    skillType: event.target.value as SkillType,
                  }))
                }
              >
                {SKILL_TYPES.map((skillType) => (
                  <option key={skillType} value={skillType}>
                    {skillType}
                  </option>
                ))}
              </select>
            </div>

            <div className="form-row">
              <label htmlFor="skill-owner">Owner</label>
              <input
                id="skill-owner"
                value={createForm.owner}
                onChange={(event) =>
                  setCreateForm((current) => ({ ...current, owner: event.target.value }))
                }
                placeholder="platform"
              />
            </div>

            <div className="form-row skills-form-row-wide">
              <label htmlFor="skill-description">Description</label>
              <textarea
                id="skill-description"
                value={createForm.description}
                onChange={(event) =>
                  setCreateForm((current) => ({
                    ...current,
                    description: event.target.value,
                  }))
                }
                rows={3}
                placeholder="What this skill governs, teaches, or constrains."
                required
              />
            </div>

            <div className="form-row skills-form-row-wide">
              <label htmlFor="skill-summary">Version summary</label>
              <textarea
                id="skill-summary"
                value={createForm.summary}
                onChange={(event) =>
                  setCreateForm((current) => ({ ...current, summary: event.target.value }))
                }
                rows={2}
                placeholder="Short summary of the initial draft."
                required
              />
            </div>

            <div className="form-row skills-form-row-wide">
              <label htmlFor="skill-body">Skill body</label>
              <textarea
                id="skill-body"
                value={createForm.body}
                onChange={(event) =>
                  setCreateForm((current) => ({ ...current, body: event.target.value }))
                }
                rows={8}
                placeholder="Structured operational context, prompts, or CLI guidance."
                required
              />
            </div>

            <div className="form-row">
              <label htmlFor="skill-tags">Tags</label>
              <input
                id="skill-tags"
                value={createForm.tags}
                onChange={(event) =>
                  setCreateForm((current) => ({ ...current, tags: event.target.value }))
                }
                placeholder="compliance, iso27001, review"
              />
            </div>

            <div className="form-row">
              <label htmlFor="skill-examples">Examples</label>
              <textarea
                id="skill-examples"
                value={createForm.examples}
                onChange={(event) =>
                  setCreateForm((current) => ({ ...current, examples: event.target.value }))
                }
                rows={3}
                placeholder="One item per line."
              />
            </div>

            <div className="form-row">
              <label htmlFor="skill-constraints">Constraints</label>
              <textarea
                id="skill-constraints"
                value={createForm.constraints}
                onChange={(event) =>
                  setCreateForm((current) => ({ ...current, constraints: event.target.value }))
                }
                rows={3}
                placeholder="One item per line."
              />
            </div>

            <div className="form-row">
              <label htmlFor="skill-source-ref">Source ref</label>
              <input
                id="skill-source-ref"
                value={createForm.sourceRef}
                onChange={(event) =>
                  setCreateForm((current) => ({ ...current, sourceRef: event.target.value }))
                }
                placeholder="docs/iso/handling.md"
              />
            </div>

            <div className="form-row">
              <label htmlFor="skill-dataset-pack">Dataset pack</label>
              <input
                id="skill-dataset-pack"
                value={createForm.datasetPackKey}
                onChange={(event) =>
                  setCreateForm((current) => ({
                    ...current,
                    datasetPackKey: event.target.value,
                  }))
                }
                placeholder="team-seed-pack-v1"
              />
            </div>

            <div className="form-row skills-form-row-wide">
              <label>Sensitivity</label>
              <div className="provider-pill-grid">
                {SENSITIVITY_LEVELS.map((level) => {
                  const active = createForm.allowedSensitivity.includes(level);
                  return (
                    <button
                      key={level}
                      type="button"
                      className={active ? "provider-pill provider-pill-active" : "provider-pill"}
                      onClick={() =>
                        setCreateForm((current) => ({
                          ...current,
                          allowedSensitivity: toggleSelection(
                            current.allowedSensitivity,
                            level,
                          ),
                        }))
                      }
                    >
                      {level}
                    </button>
                  );
                })}
              </div>
            </div>

            <div className="form-row skills-form-row-wide">
              <label htmlFor="skill-review-notes">Review notes</label>
              <textarea
                id="skill-review-notes"
                value={createForm.reviewNotes}
                onChange={(event) =>
                  setCreateForm((current) => ({
                    ...current,
                    reviewNotes: event.target.value,
                  }))
                }
                rows={2}
                placeholder="Why this draft exists and what reviewers should validate."
              />
            </div>

            <div className="skills-actions">
              <button type="submit" disabled={busyAction === "create-skill"}>
                {busyAction === "create-skill" ? "Creating..." : "Create draft skill"}
              </button>
            </div>
          </form>
        </article>

        <article className="card skills-list-panel">
          <div className="section-head">
            <div>
              <span className="eyebrow">Library</span>
              <h4>Filter and inspect</h4>
            </div>
            <span className="badge">{listLoading ? "loading" : `${skills.length} items`}</span>
          </div>

          <div className="skills-filter-grid">
            <div className="form-row">
              <label htmlFor="filter-skill-type">Type</label>
              <select
                id="filter-skill-type"
                value={filters.skillType}
                onChange={(event) =>
                  setFilters((current) => ({ ...current, skillType: event.target.value }))
                }
              >
                <option value="">All</option>
                {SKILL_TYPES.map((skillType) => (
                  <option key={skillType} value={skillType}>
                    {skillType}
                  </option>
                ))}
              </select>
            </div>

            <div className="form-row">
              <label htmlFor="filter-status">Version status</label>
              <select
                id="filter-status"
                value={filters.status}
                onChange={(event) =>
                  setFilters((current) => ({ ...current, status: event.target.value }))
                }
              >
                <option value="">All</option>
                <option value="draft">draft</option>
                <option value="review">review</option>
                <option value="approved">approved</option>
                <option value="published">published</option>
                <option value="retired">retired</option>
              </select>
            </div>

            <div className="form-row">
              <label htmlFor="filter-sensitivity">Sensitivity</label>
              <select
                id="filter-sensitivity"
                value={filters.sensitivity}
                onChange={(event) =>
                  setFilters((current) => ({ ...current, sensitivity: event.target.value }))
                }
              >
                <option value="">All</option>
                {SENSITIVITY_LEVELS.map((level) => (
                  <option key={level} value={level}>
                    {level}
                  </option>
                ))}
              </select>
            </div>

            <div className="form-row">
              <label htmlFor="filter-tag">Tag</label>
              <input
                id="filter-tag"
                value={filters.tag}
                onChange={(event) =>
                  setFilters((current) => ({ ...current, tag: event.target.value }))
                }
                placeholder="cli, compliance, routing"
              />
            </div>
          </div>

          <div className="skill-list">
            {skills.length === 0 && !listLoading ? (
              <div className="artifact-item">
                <strong>No skills match the current filters.</strong>
                <p>Clear filters or create the first governed draft from the panel on the left.</p>
              </div>
            ) : null}

            {skills.map((skill) => {
              const active = skill.id === selectedSkillId;
              return (
                <button
                  key={skill.id}
                  type="button"
                  className={active ? "workflow-list-item skill-list-item active" : "workflow-list-item skill-list-item"}
                  onClick={() => setSelectedSkillId(skill.id)}
                >
                  <div>
                    <strong>{skill.name}</strong>
                    <p>{skill.description}</p>
                    <div className="chat-index-tags">
                      <span className="chip">{skill.skill_type}</span>
                      <span className="chip">{skill.provider_exposure}</span>
                      <span className="chip">source: {skill.source_kind}</span>
                      {skill.tags.slice(0, 3).map((tag) => (
                        <span key={tag} className="chip">
                          {tag}
                        </span>
                      ))}
                    </div>
                  </div>

                  <div className="skills-list-meta">
                    <span className={`badge badge-status badge-status-${skill.latest_version_status ?? "unknown"}`}>
                      {skill.latest_version_status ?? "no-version"}
                    </span>
                    <span className="chip">{skill.assignment_count} assignments</span>
                    <span className="muted-copy">v{skill.latest_version ?? "?"}</span>
                  </div>
                </button>
              );
            })}
          </div>
        </article>

        <article className="card skills-detail-panel">
          {!selectedDetail ? (
            <div className="agent-empty-state">
              <span className="eyebrow">Detail</span>
              <h3>Select a skill</h3>
              <p className="section-copy">
                The detail panel shows version history, review events, and the current
                payload so operators can audit what is actually governable.
              </p>
            </div>
          ) : detailLoading ? (
            <div className="agent-empty-state">
              <span className="eyebrow">Detail</span>
              <h3>Loading skill detail</h3>
            </div>
          ) : (
            <>
              <div className="section-head">
                <div>
                  <span className="eyebrow">Detail</span>
                  <h4>{selectedDetail.skill.name}</h4>
                </div>
                <span className={`badge badge-status badge-status-${selectedDetail.skill.latest_version_status ?? "unknown"}`}>
                  {selectedDetail.skill.latest_version_status ?? "unknown"}
                </span>
              </div>

              <div className="artifact-item">
                <p>{selectedDetail.skill.description}</p>
                <div className="chat-index-tags">
                  <span className="chip">{selectedDetail.skill.skill_type}</span>
                  <span className="chip">{selectedDetail.skill.provider_exposure}</span>
                  <span className="chip">source: {selectedDetail.skill.source_kind}</span>
                  <span className="chip">owner: {selectedDetail.skill.owner}</span>
                  <span className="chip">{selectedDetail.skill.assignment_count} consumers</span>
                  {selectedDetail.skill.allowed_sensitivity_levels.map((level) => (
                    <span key={level} className="chip chip-blue">
                      {level}
                    </span>
                  ))}
                </div>
              </div>

              <div className="skills-detail-columns">
                <section className="tooling-block">
                  <div className="section-head">
                    <div>
                      <span className="eyebrow">Versions</span>
                      <h4>Lifecycle</h4>
                    </div>
                    <span className="badge">{selectedDetail.versions.length} versions</span>
                  </div>

                  <div className="artifact-list">
                    {selectedDetail.versions.map((version) => (
                      <article key={version.id} className="artifact-item skills-version-item">
                        <div className="section-head">
                          <div>
                            <strong>v{version.version}</strong>
                            <p>{version.summary}</p>
                          </div>
                          <span className={`badge badge-status badge-status-${version.status}`}>
                            {version.status}
                          </span>
                        </div>

                        <div className="chat-index-tags">
                          <span className="chip">author: {version.created_by}</span>
                          {version.approved_by ? (
                            <span className="chip">reviewer: {version.approved_by}</span>
                          ) : null}
                          {version.published_by ? (
                            <span className="chip">publisher: {version.published_by}</span>
                          ) : null}
                        </div>

                        {version.source_ref ? (
                          <p className="muted-copy">Source ref: {version.source_ref}</p>
                        ) : null}

                        {version.dataset_pack_key ? (
                          <p className="muted-copy">Dataset pack: {version.dataset_pack_key}</p>
                        ) : null}

                        {version.examples.length > 0 ? (
                          <ul className="stack-list">
                            {version.examples.map((example) => (
                              <li key={example}>{example}</li>
                            ))}
                          </ul>
                        ) : null}

                        <pre className="skills-body-preview">
                          {formatSkillBody(version.body) || "No body content stored."}
                        </pre>

                        <div className="skills-actions">
                          {version.status === "draft" ? (
                            <button
                              type="button"
                              disabled={busyAction === `review-${version.id}`}
                              onClick={() => handleVersionAction(version, "review")}
                            >
                              {busyAction === `review-${version.id}`
                                ? "Sending..."
                                : "Submit review"}
                            </button>
                          ) : null}

                          {version.status === "review" ? (
                            <button
                              type="button"
                              disabled={busyAction === `approve-${version.id}`}
                              onClick={() => handleVersionAction(version, "approve")}
                            >
                              {busyAction === `approve-${version.id}`
                                ? "Approving..."
                                : "Approve"}
                            </button>
                          ) : null}

                          {version.status === "approved" ? (
                            <button
                              type="button"
                              disabled={busyAction === `publish-${version.id}`}
                              onClick={() => handleVersionAction(version, "publish")}
                            >
                              {busyAction === `publish-${version.id}`
                                ? "Publishing..."
                                : "Publish"}
                            </button>
                          ) : null}
                        </div>
                      </article>
                    ))}
                  </div>
                </section>

                <section className="tooling-block">
                  <div className="section-head">
                    <div>
                      <span className="eyebrow">Compose</span>
                      <h4>New version draft</h4>
                    </div>
                    <span className="badge">manual vNext</span>
                  </div>

                  <form className="skills-form-grid" onSubmit={handleCreateVersion}>
                    <div className="form-row skills-form-row-wide">
                      <label htmlFor="version-summary">Summary</label>
                      <textarea
                        id="version-summary"
                        value={versionForm.summary}
                        onChange={(event) =>
                          setVersionForm((current) => ({
                            ...current,
                            summary: event.target.value,
                          }))
                        }
                        rows={2}
                        placeholder="What changed in this draft."
                        required
                      />
                    </div>

                    <div className="form-row skills-form-row-wide">
                      <label htmlFor="version-body">Body</label>
                      <textarea
                        id="version-body"
                        value={versionForm.body}
                        onChange={(event) =>
                          setVersionForm((current) => ({ ...current, body: event.target.value }))
                        }
                        rows={8}
                        placeholder="Updated expert context, CLI guidance, or policy text."
                        required
                      />
                    </div>

                    <div className="form-row">
                      <label htmlFor="version-examples">Examples</label>
                      <textarea
                        id="version-examples"
                        value={versionForm.examples}
                        onChange={(event) =>
                          setVersionForm((current) => ({
                            ...current,
                            examples: event.target.value,
                          }))
                        }
                        rows={3}
                        placeholder="One item per line."
                      />
                    </div>

                    <div className="form-row">
                      <label htmlFor="version-constraints">Constraints</label>
                      <textarea
                        id="version-constraints"
                        value={versionForm.constraints}
                        onChange={(event) =>
                          setVersionForm((current) => ({
                            ...current,
                            constraints: event.target.value,
                          }))
                        }
                        rows={3}
                        placeholder="One item per line."
                      />
                    </div>

                    <div className="form-row">
                      <label htmlFor="version-source-ref">Source ref</label>
                      <input
                        id="version-source-ref"
                        value={versionForm.sourceRef}
                        onChange={(event) =>
                          setVersionForm((current) => ({
                            ...current,
                            sourceRef: event.target.value,
                          }))
                        }
                        placeholder="docs/skills/cli-ingestion.md"
                      />
                    </div>

                    <div className="form-row">
                      <label htmlFor="version-dataset-pack">Dataset pack</label>
                      <input
                        id="version-dataset-pack"
                        value={versionForm.datasetPackKey}
                        onChange={(event) =>
                          setVersionForm((current) => ({
                            ...current,
                            datasetPackKey: event.target.value,
                          }))
                        }
                        placeholder="shared-enterprise-pack"
                      />
                    </div>

                    <div className="form-row skills-form-row-wide">
                      <label htmlFor="version-review-notes">Review notes</label>
                      <textarea
                        id="version-review-notes"
                        value={versionForm.reviewNotes}
                        onChange={(event) =>
                          setVersionForm((current) => ({
                            ...current,
                            reviewNotes: event.target.value,
                          }))
                        }
                        rows={2}
                        placeholder="What reviewers should focus on before promotion."
                      />
                    </div>

                    <div className="skills-actions">
                      <button type="submit" disabled={busyAction === "create-version"}>
                        {busyAction === "create-version" ? "Creating..." : "Create new draft"}
                      </button>
                    </div>
                  </form>
                </section>
              </div>

              <section className="tooling-block">
                <div className="section-head">
                  <div>
                    <span className="eyebrow">Assignments</span>
                    <h4>Published consumers</h4>
                  </div>
                  <span className="badge">{selectedDetail.assignments.length} linked</span>
                </div>

                {publishedVersions.length === 0 ? (
                  <p className="muted-copy">
                    Publish a version before assigning this skill to templates, roles, or providers.
                  </p>
                ) : (
                  <form className="skills-assignment-form" onSubmit={handleCreateAssignment}>
                    <div className="form-row">
                      <label htmlFor="assignment-version">Published version</label>
                      <select
                        id="assignment-version"
                        value={assignmentForm.skillVersionId}
                        onChange={(event) =>
                          setAssignmentForm((current) => ({
                            ...current,
                            skillVersionId: event.target.value,
                          }))
                        }
                      >
                        {publishedVersions.map((version) => (
                          <option key={version.id} value={version.id}>
                            v{version.version} - {version.summary}
                          </option>
                        ))}
                      </select>
                    </div>

                    <div className="form-row">
                      <label htmlFor="assignment-target-type">Target type</label>
                      <select
                        id="assignment-target-type"
                        value={assignmentForm.targetType}
                        onChange={(event) => {
                          const nextType = event.target.value as AssignmentFormState["targetType"];
                          const nextKeys = assignmentOptionsForType(assignmentTargets, nextType);
                          setAssignmentForm((current) => ({
                            ...current,
                            targetType: nextType,
                            targetKey: nextKeys[0] ?? "",
                          }));
                        }}
                      >
                        <option value="agent_role">agent role</option>
                        <option value="workflow_template">workflow template</option>
                        <option value="provider">provider</option>
                      </select>
                    </div>

                    <div className="form-row skills-form-row-wide">
                      <label htmlFor="assignment-target-key">Target</label>
                      <select
                        id="assignment-target-key"
                        value={assignmentForm.targetKey}
                        onChange={(event) =>
                          setAssignmentForm((current) => ({
                            ...current,
                            targetKey: event.target.value,
                          }))
                        }
                      >
                        {assignmentOptionsForType(
                          assignmentTargets,
                          assignmentForm.targetType,
                        ).map((option) => (
                          <option key={option} value={option}>
                            {option}
                          </option>
                        ))}
                      </select>
                    </div>

                    <div className="skills-actions">
                      <button type="submit" disabled={busyAction === "create-assignment"}>
                        {busyAction === "create-assignment"
                          ? "Linking..."
                          : "Create assignment"}
                      </button>
                    </div>
                  </form>
                )}

                <div className="artifact-list">
                  {selectedDetail.assignments.map((assignment) => (
                    <article key={assignment.id} className="artifact-item review-timeline-item">
                      <div className="section-head">
                        <div>
                          <strong>{assignment.target_key}</strong>
                          <p>
                            {assignment.target_type} - v{assignment.skill_version}
                          </p>
                        </div>
                        <button
                          type="button"
                          className="ghost"
                          disabled={busyAction === `delete-assignment-${assignment.id}`}
                          onClick={() => void handleDeleteAssignment(assignment.id)}
                        >
                          {busyAction === `delete-assignment-${assignment.id}`
                            ? "Removing..."
                            : "Remove"}
                        </button>
                      </div>
                    </article>
                  ))}
                  {selectedDetail.assignments.length === 0 ? (
                    <p className="muted-copy">
                      No assignments yet. Skills stay governable but unused until linked to a
                      template, role, or provider.
                    </p>
                  ) : null}
                </div>
              </section>

              <section className="tooling-block">
                <div className="section-head">
                  <div>
                    <span className="eyebrow">Audit</span>
                    <h4>Review events</h4>
                  </div>
                  <span className="badge">{selectedDetail.reviews.length} events</span>
                </div>

                <div className="review-timeline">
                  {selectedDetail.reviews.map((review) => (
                    <article key={review.id} className="artifact-item review-timeline-item">
                      <div className="section-head">
                        <strong>{review.action}</strong>
                        <span className="badge">{review.actor_role}</span>
                      </div>
                      <p>
                        {review.actor_name}
                        {review.comment ? ` - ${review.comment}` : ""}
                      </p>
                    </article>
                  ))}
                </div>
              </section>
            </>
          )}
        </article>
      </section>
    </section>
  );
}

function SkillMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="agent-metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function slugify(value: string) {
  return value
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function normalizeOptionalText(value: string) {
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function parseDelimitedList(value: string) {
  return value
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function toggleSelection(current: string[], value: string) {
  const next = current.includes(value)
    ? current.filter((item) => item !== value)
    : [...current, value];
  return next.length > 0 ? next : current;
}

function defaultProviderExposure(skillType: SkillType): SkillProviderExposure {
  if (skillType === "agent-context") return "agent_context_only";
  if (skillType === "provider") return "provider_allowed";
  return "local_only";
}

function assignmentOptionsForType(
  targets: SkillAssignmentTargets | null,
  targetType: AssignmentFormState["targetType"],
) {
  if (!targets) return [];
  if (targetType === "workflow_template") return targets.workflow_templates;
  if (targetType === "provider") return targets.providers.map(String);
  return targets.agent_roles;
}

function formatSkillBody(body: Record<string, unknown>) {
  const content = body.content;
  if (typeof content === "string") {
    return content;
  }
  return JSON.stringify(body, null, 2);
}

function normalizeError(error: unknown) {
  if (error instanceof Error) return error.message;
  return "Unexpected error";
}
