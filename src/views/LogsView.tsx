import { useEffect, useRef } from "react";
import type { BuildLauncher } from "../hooks/useBuildLauncher";
import type { LogEvent } from "../types";
import { Button, LogLine, Panel } from "../components/ui";
const levels: Array<"all" | LogEvent["level"]> = ["all", "group", "info", "success", "warn", "error"];

export function LogsView({ launcher }: { launcher: BuildLauncher }) {
  const endRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (launcher.autoScrollLogs) {
      endRef.current?.scrollIntoView({ block: "end" });
    }
  }, [launcher.autoScrollLogs, launcher.filteredLogs.length]);

  return (
    <Panel
      title="Build Console"
      kicker={`${launcher.filteredLogs.length} shown / ${launcher.logs.length} retained`}
      actions={(
        <div className="button-cluster">
          <Button variant="ghost" onClick={() => launcher.setAutoScrollLogs(!launcher.autoScrollLogs)}>
            {launcher.autoScrollLogs ? "Auto-scroll on" : "Auto-scroll off"}
          </Button>
          <Button variant="danger" disabled={!launcher.busy} onClick={launcher.cancelBuild}>Cancel</Button>
        </div>
      )}
    >
      <div className="log-toolbar">
        <input
          className="input"
          value={launcher.logSearch}
          onChange={(event) => launcher.setLogSearch(event.target.value)}
          placeholder="Search logs"
        />
        <div className="segmented-control" aria-label="Log level">
          {levels.map((level) => (
            <button
              key={level}
              className={launcher.logLevel === level ? "active" : ""}
              onClick={() => launcher.setLogLevel(level)}
            >
              {level}
            </button>
          ))}
        </div>
      </div>
      <div className="log-box">
        {launcher.filteredLogs.map((log, index) => <LogLine key={`${log.buildId}-${index}`} log={log} />)}
        {!launcher.filteredLogs.length && <span className="empty-log">Build logs will appear here.</span>}
        <div ref={endRef} />
      </div>
    </Panel>
  );
}
