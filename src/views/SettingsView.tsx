import { useEffect, useState } from "react";
import type { useBuildLauncher } from "../hooks/useBuildLauncher";
import type { AppConfig, ShellMode } from "../types";
import { Button, Field, Icon, Panel, ToolRow } from "../components/ui";
import { api } from "../tauri";

type Launcher = ReturnType<typeof useBuildLauncher>;

export function SettingsView({ launcher }: { launcher: Launcher }) {
  const [draft, setDraft] = useState<AppConfig>(launcher.config);

  useEffect(() => {
    setDraft(launcher.config);
  }, [launcher.config]);

  return (
    <div className="settings-grid">
      <Panel title="Settings" kicker="Local preferences">
        <div className="form-grid">
          <Field label="Default repo folder">
            <div className="input-row">
              <input className="input" value={draft.defaultRepoFolder} onChange={(event) => setDraft({ ...draft, defaultRepoFolder: event.target.value })} />
              <Button variant="secondary" onClick={async () => {
                const selected = await api.chooseFolder();
                if (selected) {
                  setDraft({ ...draft, defaultRepoFolder: selected });
                }
              }} icon={<Icon name="folder" />}>Browse</Button>
            </div>
          </Field>

          <Field label="Default output folder">
            <div className="input-row">
              <input className="input" value={draft.defaultOutputFolder} onChange={(event) => setDraft({ ...draft, defaultOutputFolder: event.target.value })} />
              <Button variant="secondary" onClick={async () => {
                const selected = await api.chooseFolder();
                if (selected) {
                  setDraft({ ...draft, defaultOutputFolder: selected });
                }
              }} icon={<Icon name="folder" />}>Browse</Button>
            </div>
          </Field>

          <Field label="Default shell mode">
            <select className="input" value={draft.shellMode} onChange={(event) => setDraft({ ...draft, shellMode: event.target.value as ShellMode })}>
              <option value="native">Native Windows</option>
              <option value="bash">Git Bash</option>
            </select>
          </Field>

          <Field label="Theme">
            <select className="input" value={draft.theme} onChange={(event) => setDraft({ ...draft, theme: event.target.value as AppConfig["theme"] })}>
              <option value="system">System</option>
              <option value="dark">Dark</option>
              <option value="light">Light</option>
            </select>
          </Field>
        </div>

        <div className="settings-actions">
          <Button onClick={() => launcher.saveSettings(draft)}>Save Settings</Button>
          <Button variant="ghost" onClick={() => setDraft(launcher.config)}>Cancel</Button>
        </div>
      </Panel>

      <Panel
        title="Tools"
        kicker={launcher.toolStatus?.toolsRoot ?? "Checking"}
        actions={(
          <div className="button-cluster">
            <Button variant="secondary" onClick={launcher.refreshTools} icon={<Icon name="refresh" />}>Refresh</Button>
            <Button disabled={launcher.busy || launcher.installingTools} onClick={launcher.installTools}>Install / Repair</Button>
          </div>
        )}
      >
        <div className="tool-grid">
          <ToolRow label="Git" probe={launcher.toolStatus?.git} />
          <ToolRow label="Java 17" probe={launcher.toolStatus?.java} />
          <ToolRow label="Android SDK" probe={launcher.toolStatus?.androidSdk} />
          <ToolRow label="Git Bash" probe={launcher.toolStatus?.gitBash} />
        </div>
      </Panel>
    </div>
  );
}
