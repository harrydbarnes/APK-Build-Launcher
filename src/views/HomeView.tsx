import type { useBuildLauncher } from "../hooks/useBuildLauncher";
import type { BuildPreset, BuildTarget, ShellMode, SourceMode } from "../types";
import { Button, Field, Icon, Panel, ReadinessBadge, ToolRow } from "../components/ui";

type Launcher = ReturnType<typeof useBuildLauncher>;

export function HomeView({ launcher }: { launcher: Launcher }) {
  const {
    branches,
    branchMessage,
    busy,
    config,
    draft,
    installingTools,
    analyzing,
    plan,
    loadingBranches,
    readiness,
    savedSecretNames,
    secretDraft,
    selectedJob,
    selectedPresetId,
    toolStatus,
    workflows,
  } = launcher;

  return (
    <div className="home-grid">
      <section className="hero-panel">
        <div>
          <h2>Build APKs and Windows apps locally</h2>
          <p>Use a Git ref or your current local folder. The launcher analyses compatibility, installs portable per-user tools, and publishes verified artifacts without consuming Actions quota.</p>
        </div>
        <div className="hero-actions">
          <Button disabled={!readiness.canBuild} onClick={launcher.startBuild} icon={<Icon name="play" />}>
            Build locally
          </Button>
          <Button variant="secondary" disabled={!readiness.canDetect} onClick={launcher.analyzeCurrent} icon={<Icon name="workflow" />}>
            {analyzing ? "Analysing" : "Analyse Plan"}
          </Button>
          <Button variant="ghost" disabled={busy || installingTools} onClick={launcher.installTools} icon={<Icon name="refresh" />}>
            {installingTools ? "Installing" : "Repair Tools"}
          </Button>
        </div>
      </section>

      <Panel title="Build Setup" kicker="Current run">
        <div className="form-grid">
          <Field label="Project source">
            <select
              className="input"
              value={draft.sourceMode}
              onChange={(event) => launcher.updateDraft({ sourceMode: event.target.value as SourceMode })}
            >
              <option value="local">Existing local folder — includes uncommitted changes</option>
              <option value="remote">Git repository — branch, tag, commit, or PR</option>
            </select>
          </Field>

          {draft.sourceMode === "remote" ? <Field label="Git repository URL" hint={branchMessage}>
            <div className="input-row">
              <input
                className="input"
                value={draft.repoUrl}
                onChange={(event) => launcher.updateDraft({ repoUrl: event.target.value })}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    void launcher.loadBranches();
                  }
                }}
                placeholder="https://github.com/org/repo.git"
              />
              <Button variant="secondary" disabled={busy || loadingBranches || !draft.repoUrl.trim()} onClick={launcher.loadBranches}>
                {loadingBranches ? "Checking" : "Branches"}
              </Button>
            </div>
          </Field> : <Field label="Local project folder" hint="A clean isolated snapshot is built; the selected folder is never modified.">
            <div className="input-row">
              <input
                className="input"
                value={draft.localPath}
                onChange={(event) => launcher.updateDraft({ localPath: event.target.value })}
                placeholder="C:\Dev\MyProject"
              />
              <Button variant="secondary" onClick={launcher.chooseLocalFolder} icon={<Icon name="folder" />}>Browse</Button>
            </div>
          </Field>}

          {draft.sourceMode === "remote" && <Field label="Branch, tag, commit, or PR number">
            <input
              className="input"
              list="branch-options"
              value={draft.refName}
              onChange={(event) => launcher.updateDraft({ refName: event.target.value })}
              placeholder="dev or 123"
            />
            <datalist id="branch-options">
              {branches.map((branch) => <option key={branch} value={branch} />)}
            </datalist>
          </Field>}

          <Field label="Build target">
            <select className="input" value={draft.target} onChange={(event) => launcher.updateDraft({ target: event.target.value as BuildTarget })}>
              <option value="auto">Detect automatically</option>
              <option value="android">Android APK / AAB</option>
              <option value="tauri">Tauri Windows EXE / MSI</option>
              <option value="electron">Electron Windows EXE</option>
              <option value="dotnet">.NET Windows EXE</option>
            </select>
          </Field>

          <Field label="Workflow">
            <select
              className="input"
              value={draft.workflowPath}
              onChange={(event) => {
                const workflow = workflows.find((item) => item.filePath === event.target.value);
                launcher.updateDraft({ workflowPath: event.target.value, jobId: workflow?.jobs[0]?.id ?? "" });
              }}
            >
              <option value="">Native project plan (recommended)</option>
              {workflows.map((workflow) => <option key={workflow.filePath} value={workflow.filePath}>{workflow.name}</option>)}
            </select>
          </Field>

          <Field label="Job">
            <select className="input" value={draft.jobId} onChange={(event) => launcher.updateDraft({ jobId: event.target.value })}>
              <option value="">No workflow job</option>
              {launcher.selectedWorkflow?.jobs.map((job) => <option key={job.id} value={job.id}>{job.name}</option>)}
            </select>
          </Field>

          <Field label="Output folder">
            <div className="input-row">
              <input className="input" value={draft.outputFolder} onChange={(event) => launcher.updateDraft({ outputFolder: event.target.value })} />
              <Button variant="secondary" onClick={launcher.chooseOutputFolder} icon={<Icon name="folder" />}>Browse</Button>
            </div>
          </Field>

          <Field label="Compatibility mode">
            <select className="input" value={draft.shellMode} onChange={(event) => launcher.updateDraft({ shellMode: event.target.value as ShellMode })}>
              <option value="native">Native Windows</option>
              <option value="bash">Git Bash</option>
            </select>
          </Field>
        </div>

        {selectedJob && (
          <div className="selected-job">
            <strong>{selectedJob.name}</strong>
            <span>{selectedJob.runsOn}</span>
            <span>{selectedJob.stepCount} steps</span>
          </div>
        )}
      </Panel>

      {plan && (
        <Panel title="Local Build Plan" kicker={plan.targetLabel}>
          <div className={`plan-summary ${plan.supported ? "ready" : "blocked"}`}>
            <strong>{plan.summary}</strong>
            <span>{plan.steps.filter((step) => step.disposition === "run").length} command step(s), {plan.tools.length} tool requirement(s), {plan.artifacts.length} artifact rule(s)</span>
          </div>
          {!!plan.blockers.length && <div className="plan-messages blockers">{plan.blockers.map((message) => <p key={message}>{message}</p>)}</div>}
          {!!plan.warnings.length && <div className="plan-messages warnings">{plan.warnings.map((message) => <p key={message}>{message}</p>)}</div>}
          <div className="plan-step-list">
            {plan.steps.map((step, index) => (
              <div className={`plan-step ${step.disposition}`} key={`${step.name}-${index}`}>
                <span>{step.disposition}</span>
                <div>
                  <strong>{step.name}</strong>
                  <p>{step.reason}</p>
                  {step.command && <code className="plan-command">{step.command}</code>}
                </div>
              </div>
            ))}
          </div>
          <p className="trust-note">Build commands run with your Windows user permissions. Only build source and workflow files you trust.</p>
        </Panel>
      )}

      <Panel
        title="Presets"
        kicker={`${config.presets.length} saved`}
        actions={(
          <div className="button-cluster">
            <Button variant="secondary" disabled={draft.sourceMode === "local" ? !draft.localPath.trim() : !draft.repoUrl.trim()} onClick={launcher.saveCurrentAsPreset} icon={<Icon name="save" />}>New</Button>
            <Button variant="ghost" disabled={!selectedPresetId} onClick={launcher.updateSelectedPreset}>Update</Button>
          </div>
        )}
      >
        <div className="preset-list">
          {config.presets.map((preset) => (
            <PresetRow
              key={preset.id}
              preset={preset}
              active={preset.id === selectedPresetId}
              isDefault={preset.id === config.defaultPresetId}
              onSelect={() => launcher.selectPreset(preset)}
              onDuplicate={() => launcher.duplicatePreset(preset)}
              onDefault={() => launcher.setDefaultPreset(preset.id)}
              onDelete={() => launcher.deletePreset(preset.id)}
              onRename={(name) => launcher.renamePreset(preset.id, name)}
            />
          ))}
          {!config.presets.length && <p className="empty-note">Save the current setup as a preset to make repeat builds one click away.</p>}
        </div>
      </Panel>

      <Panel title="Readiness" kicker="Run checks">
        <div className="readiness-grid">
          <ReadinessBadge label="Project source" ready={readiness.repoReady} />
          <ReadinessBadge label="Workflow" ready={readiness.workflowReady} />
          <ReadinessBadge label="Job" ready={readiness.jobReady} />
          <ReadinessBadge label="Output folder" ready={readiness.outputReady} />
          <ReadinessBadge label="Secrets" ready={readiness.secretsReady} />
          <ReadinessBadge label="Build plan" ready={readiness.planReady} />
          <ReadinessBadge label="Shell mode" ready={readiness.bashReady} />
        </div>
        <div className="tool-grid compact">
          <ToolRow label="Git" probe={toolStatus?.git} />
          <ToolRow label="Java 17" probe={toolStatus?.java} />
          <ToolRow label="Android SDK" probe={toolStatus?.androidSdk} />
          <ToolRow label="Git Bash" probe={toolStatus?.gitBash} />
          <ToolRow label="Node.js" probe={toolStatus?.node} />
          <ToolRow label="Rust + linker" probe={toolStatus?.rust} />
          <ToolRow label=".NET SDK" probe={toolStatus?.dotnet} />
        </div>
      </Panel>

      <Panel title="Secrets" kicker="Stored per repo">
        <div className="secret-grid">
          {(plan?.requiredSecrets ?? []).map((name) => (
            <Field key={name} label={`${name}${savedSecretNames.includes(name) ? " saved" : ""}`}>
              <textarea
                className="input secret-input"
                value={secretDraft[name] ?? ""}
                onChange={(event) => launcher.setSecretDraft({ ...secretDraft, [name]: event.target.value })}
              />
            </Field>
          ))}
          {!plan?.requiredSecrets.length && <p className="empty-note">Analyse the build plan to discover required secrets. Projects without workflow secrets need no setup here.</p>}
        </div>
        <Button disabled={busy || !plan?.requiredSecrets.length} onClick={launcher.saveSecrets}>Save Secrets</Button>
      </Panel>
    </div>
  );
}

function PresetRow({
  active,
  isDefault,
  preset,
  onDefault,
  onDelete,
  onDuplicate,
  onRename,
  onSelect,
}: {
  active: boolean;
  isDefault: boolean;
  preset: BuildPreset;
  onDefault: () => void;
  onDelete: () => void;
  onDuplicate: () => void;
  onRename: (name: string) => void;
  onSelect: () => void;
}) {
  return (
    <div className={`preset-row ${active ? "active" : ""}`}>
      <button className="preset-main" onClick={onSelect}>
        <strong>{preset.name}</strong>
        <span>{preset.refName} | {preset.shellMode} | {preset.workflowPath ? "workflow saved" : "workflow pending"}</span>
      </button>
      <div className="icon-actions">
        <button title="Set default" className={isDefault ? "active" : ""} onClick={onDefault}><Icon name="star" /></button>
        <button title="Rename" onClick={() => {
          const name = window.prompt("Preset name", preset.name);
          if (name) {
            onRename(name);
          }
        }}>Aa</button>
        <button title="Duplicate" onClick={onDuplicate}><Icon name="copy" /></button>
        <button title="Delete" onClick={() => {
          if (window.confirm(`Delete ${preset.name}?`)) {
            onDelete();
          }
        }}><Icon name="trash" /></button>
      </div>
    </div>
  );
}
