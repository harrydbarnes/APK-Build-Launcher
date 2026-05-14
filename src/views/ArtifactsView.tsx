import type { BuildLauncher } from "../hooks/useBuildLauncher";
import { Button, Icon, Panel } from "../components/ui";

export function ArtifactsView({ launcher }: { launcher: BuildLauncher }) {
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
                <h3>{launcher.result.apkFiles.length} APK file{launcher.result.apkFiles.length === 1 ? "" : "s"} copied</h3>
                <p>{launcher.result.outputFolder}</p>
              </div>
              <Button variant="secondary" onClick={() => copyText(launcher.result?.outputFolder ?? "")} icon={<Icon name="copy" />}>
                Copy Output
              </Button>
            </div>

            <div className="artifact-list">
              {launcher.result.apkFiles.map((file) => (
                <div className="artifact-row" key={file}>
                  <span>{file}</span>
                  <Button variant="ghost" onClick={() => copyText(file)} icon={<Icon name="copy" />}>Copy</Button>
                </div>
              ))}
            </div>

            {launcher.latestPath && (
              <div className="latest-path">
                <strong>Latest folder</strong>
                <span>{launcher.latestPath}</span>
                <Button variant="ghost" onClick={() => copyText(launcher.latestPath)} icon={<Icon name="copy" />}>Copy</Button>
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
    </div>
  );
}
