import type { BuildLauncher } from "../hooks/useBuildLauncher";
import type { BuildPreset, ShellMode } from "../types";
import { Button, Field, Icon, Panel, ReadinessBadge, ToolRow } from "../components/ui";

const secretNames = ["LOCAL_PROPERTIES_BASE64", "LOCAL_DEV_PROPERTIES_BASE64"];

export function HomeView({ launcher }: { launcher: BuildLauncher }) {
  const {
    branches,
    branchMessage,
    busy,
    config,
    draft,
    installingTools,
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
          <h2>Build Android APKs from saved workflows</h2>
          <p>Pick a preset or prepare a repo, confirm readiness, then launch the local adapter with clearer logs and artifact history.</p>
        </div>
        <div className="hero-actions">
          <Button disabled={!readiness.canBuild} onClick={launcher.startBuild} icon={<Icon name="play" />}>
            Build APK
          </Button>
          <Button variant="secondary" disabled={!readiness.canDetect} onClick={launcher.prepareAndDetect} icon={<Icon name="workflow" />}>
            Detect Workflows
          </Button>
          <Button variant="ghost" disabled={busy || installingTools} onClick={launcher.installTools} icon={<Icon name="refresh" />}>
            {installingTools ? "Installing" : "Repair Tools"}
          </Button>
        </div>
      </section>

      <Panel title="Build Setup" kicker="Current run">
        <div className="form-grid">
          <Field label="GitHub repo URL" hint={branchMessage}>
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
          </Field>

          <Field label="Branch, PR branch, or PR number">
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
              <option value="">Detect workflows first</option>
              {workflows.map((workflow) => <option key={workflow.filePath} value={workflow.filePath}>{workflow.name}</option>)}
            </select>
          </Field>

          <Field label="Job">
            <select className="input" value={draft.jobId} onChange={(event) => launcher.updateDraft({ jobId: event.target.value })}>
              <option value="">Select a workflow first</option>
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

      <Panel
        title="Presets"
        kicker={`${config.presets.length} saved`}
        actions={(
          <div className="button-cluster">
            <Button variant="secondary" disabled={!draft.repoUrl.trim()} onClick={launcher.saveCurrentAsPreset} icon={<Icon name="save" />}>New</Button>
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
          <ReadinessBadge label="Repo and ref" ready={readiness.repoReady} />
          <ReadinessBadge label="Workflow" ready={readiness.workflowReady} />
          <ReadinessBadge label="Job" ready={readiness.jobReady} />
          <ReadinessBadge label="Output folder" ready={readiness.outputReady} />
          <ReadinessBadge label="Optional secrets" ready={readiness.secretsReady} />
          <ReadinessBadge label="Shell mode" ready={readiness.bashReady} />
        </div>
        <div className="tool-grid compact">
          <ToolRow label="Git" probe={toolStatus?.git} />
          <ToolRow label="Java 17" probe={toolStatus?.java} />
          <ToolRow label="Android SDK" probe={toolStatus?.androidSdk} />
          <ToolRow label="Git Bash" probe={toolStatus?.gitBash} />
        </div>
      </Panel>

      <Panel title="Secrets" kicker="Stored per repo, optional">
        <div className="secret-grid">
          {secretNames.map((name) => (
            <Field key={name} label={`${name}${savedSecretNames.includes(name) ? " saved" : ""}`}>
              <textarea
                className="input secret-input"
                value={secretDraft[name] ?? ""}
                onChange={(event) => launcher.setSecretDraft({ ...secretDraft, [name]: event.target.value })}
              />
            </Field>
          ))}
        </div>
        <Button disabled={busy || !draft.repoUrl.trim()} onClick={launcher.saveSecrets}>Save Secrets</Button>
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
