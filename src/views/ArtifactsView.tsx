import type { useBuildLauncher } from "../hooks/useBuildLauncher";
import { Button, Icon, Panel } from "../components/ui";

type Launcher = ReturnType<typeof useBuildLauncher>;

export function ArtifactsView({ launcher }: { launcher: Launcher }) {
  const copyText = async (text: string) => {
    await navigator.clipboard?.writeText(text);
  };

  return (
    <div className="view-grid">
      <Panel title="Artifacts" kicker={launcher.result ? launcher.result.buildId : "No completed build yet"}>
        {launcher.result ? (
          <div className="artifact-stack">
            <div className="artifact-summary">
              <div>
                <h3>{launcher.result.artifacts.length} verified artifact{launcher.result.artifacts.length === 1 ? "" : "s"} published</h3>
                <p>{launcher.result.outputFolder}</p>
              </div>
              <Button variant="secondary" onClick={() => launcher.openFolder(launcher.result?.outputFolder ?? "")} icon={<Icon name="folder" />}>
                Open Output
              </Button>
            </div>

            <div className="artifact-list">
              {launcher.result.artifacts.map((artifact) => (
                <div className="artifact-row" key={artifact.path}>
                  <span><strong>{artifact.kind.toUpperCase()}</strong> {artifact.relativePath}<small>{(artifact.sizeBytes / 1024 / 1024).toFixed(1)} MB · SHA-256 {artifact.sha256.slice(0, 12)}…</small></span>
                  <Button variant="ghost" onClick={() => copyText(artifact.path)} icon={<Icon name="copy" />}>Copy</Button>
                </div>
              ))}
            </div>

            {launcher.latestPath && (
              <div className="latest-path">
                <strong>Latest folder</strong>
                <span>{launcher.latestPath}</span>
                <Button variant="ghost" onClick={() => launcher.openFolder(launcher.latestPath)} icon={<Icon name="folder" />}>Open</Button>
              </div>
            )}
          </div>
        ) : (
          <div className="empty-state">
            <Icon name="artifact" />
            <h3>No artifacts yet</h3>
            <p>Completed builds will list copied APKs here with output and latest paths.</p>
          </div>
        )}
      </Panel>

      <Panel title="Build History" kicker={`${launcher.history.length} retained manifest${launcher.history.length === 1 ? "" : "s"}`}>
        <div className="artifact-list">
          {launcher.history.map((build) => (
            <div className="artifact-row" key={build.buildId}>
              <span><strong>{build.target}</strong> {new Date(build.createdAt).toLocaleString()}<small>{build.revision.slice(0, 12)} · {build.artifacts.length} artifact(s)</small></span>
              <Button variant="ghost" onClick={() => launcher.openFolder(build.outputFolder)} icon={<Icon name="folder" />}>Open</Button>
            </div>
          ))}
          {!launcher.history.length && <p className="empty-note">Completed builds write manifests here so results remain discoverable after restart.</p>}
        </div>
      </Panel>
    </div>
  );
}
