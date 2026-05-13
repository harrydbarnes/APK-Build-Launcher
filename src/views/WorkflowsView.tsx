import type { useBuildLauncher } from "../hooks/useBuildLauncher";
import { Button, Icon, Panel } from "../components/ui";

type Launcher = ReturnType<typeof useBuildLauncher>;

export function WorkflowsView({ launcher }: { launcher: Launcher }) {
  return (
    <div className="view-grid">
      <Panel
        title="Detected Workflows"
        kicker={launcher.repoPath || "No repository prepared"}
        actions={(
          <Button variant="secondary" disabled={!launcher.readiness.canDetect} onClick={launcher.prepareAndDetect} icon={<Icon name="refresh" />}>
            Refresh
          </Button>
        )}
      >
        <div className="workflow-list">
          {launcher.workflows.map((workflow) => (
            <article key={workflow.filePath} className={`workflow-card ${workflow.filePath === launcher.draft.workflowPath ? "active" : ""}`}>
              <div className="workflow-topline">
                <div>
                  <h3>{workflow.name}</h3>
                  <p>{workflow.filePath}</p>
                  <small>Trigger: {workflow.trigger}</small>
                </div>
                <Button
                  variant={workflow.filePath === launcher.draft.workflowPath ? "primary" : "secondary"}
                  onClick={() => launcher.updateDraft({ workflowPath: workflow.filePath, jobId: workflow.jobs[0]?.id ?? "" })}
                >
                  Use
                </Button>
              </div>
              <div className="job-list">
                {workflow.jobs.map((job) => (
                  <button
                    key={job.id}
                    className={job.id === launcher.draft.jobId && workflow.filePath === launcher.draft.workflowPath ? "active" : ""}
                    onClick={() => launcher.updateDraft({ workflowPath: workflow.filePath, jobId: job.id })}
                  >
                    <strong>{job.name}</strong>
                    <span>{job.runsOn}</span>
                    <span>{job.stepCount} steps</span>
                  </button>
                ))}
              </div>
            </article>
          ))}
          {!launcher.workflows.length && (
            <div className="empty-state">
              <Icon name="workflow" />
              <h3>No workflows detected</h3>
              <p>Prepare the repository to inspect `.github/workflows` and choose a job for the build adapter.</p>
            </div>
          )}
        </div>
      </Panel>
    </div>
  );
}
