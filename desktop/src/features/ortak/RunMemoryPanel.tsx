import { Alert, AlertDescription, AlertTitle } from "@/shared/ui/alert";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/shared/ui/card";
import type { ActivityText, RunMemory } from "./types";

function NoteText({ content }: { content: ActivityText }) {
  return (
    <>
      <pre className="whitespace-pre-wrap break-words font-mono text-xs">
        {content.text}
      </pre>
      {content.redacted ? (
        <p className="text-xs text-muted-foreground">
          Sensitive text was redacted.
        </p>
      ) : null}
    </>
  );
}

export function RunMemoryPanel({ memory }: { memory: RunMemory }) {
  const write = memory.write;
  const reviewed = memory.reviewed ?? [];
  const conversation = memory.scope === "run_scratch_and_reviewed_conversation";
  const employee = memory.scope === "run_scratch_and_reviewed_employee";
  const scoped = conversation || employee;
  return (
    <Card aria-label="Run memory">
      <CardHeader>
        <CardTitle>
          <h2>Memory</h2>
        </CardTitle>
        <CardDescription>Notes used and saved for this run</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <section className="flex flex-col gap-2" aria-label="Memory used">
          <h3 className="text-sm font-medium">Included before starting</h3>
          {memory.recall.withheld ? (
            <p className="text-sm text-muted-foreground">
              {employee
                ? "Previously included notes are withheld because the requester or current memory permissions do not permit this view."
                : "Previously included notes are withheld because this run’s conversation authority is no longer current."}
            </p>
          ) : memory.recall.status === "not_prepared" ? (
            <p className="text-sm text-muted-foreground">
              Memory context has not been prepared.
            </p>
          ) : memory.recall.records.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              No earlier notes were included.
            </p>
          ) : (
            <ul
              className="flex flex-col gap-3"
              aria-label="Included memory notes"
            >
              {memory.recall.records.map((record) => (
                <li key={record.record_ref} className="flex flex-col gap-1">
                  <NoteText content={record.content} />
                  <p className="break-all text-xs text-muted-foreground">
                    Source: {record.source}
                  </p>
                  <time
                    className="text-xs text-muted-foreground"
                    dateTime={record.recorded_at}
                  >
                    {new Date(record.recorded_at).toLocaleString()}
                  </time>
                </li>
              ))}
            </ul>
          )}
          {memory.recall.truncated ? (
            <p className="text-xs text-muted-foreground">
              Only the notes that fit this run’s context limit were included.
            </p>
          ) : null}
        </section>
        {reviewed.length > 0 ? (
          <section
            className="flex flex-col gap-2"
            aria-label={
              employee
                ? "Reviewed employee and mixed memory used"
                : conversation
                  ? "Reviewed memory with conversation facts"
                  : "Reviewed project memory used"
            }
          >
            <h3 className="text-sm font-medium">
              {employee
                ? "Approved employee and shared facts included before starting"
                : conversation
                  ? "Approved facts for this conversation context"
                  : "Approved project facts included before starting"}
            </h3>
            {scoped ? (
              <p className="text-xs text-muted-foreground">
                {employee
                  ? "Employee, relationship, conversation and project facts retain their original order and audiences."
                  : "Conversation facts and any included project facts retain their original order."}
              </p>
            ) : null}
            <ul className="flex flex-col gap-3">
              {reviewed.map((record) => (
                <li key={record.fact_id} className="flex flex-col gap-1">
                  {record.current &&
                  record.content &&
                  !memory.recall.withheld ? (
                    <NoteText content={record.content} />
                  ) : (
                    <p className="text-sm text-muted-foreground">
                      Use is no longer permitted. Text is withheld; the original
                      use receipt remains.
                    </p>
                  )}
                  <p className="break-all text-xs text-muted-foreground">
                    Reviewed fact: {record.fact_id}
                  </p>
                  {scoped && record.current && !memory.recall.withheld ? (
                    <div className="flex flex-col gap-1 text-xs text-muted-foreground">
                      {record.audience_kind === "project" ? (
                        <p>Audience: project</p>
                      ) : record.audience_kind === "employee" &&
                        record.audience &&
                        "destination_channel_id" in record.audience ? (
                        <>
                          <p>
                            Audience:{" "}
                            {record.audience.kind === "relationship"
                              ? "this human and employee relationship"
                              : "employee experience in this channel"}
                          </p>
                          <p className="break-all">
                            Employee: {record.audience.employee_id}
                          </p>
                          <p className="break-all">
                            Destination:{" "}
                            {record.audience.destination_channel_id}
                          </p>
                          {record.audience.human_public_key ? (
                            <p className="break-all">
                              Human: {record.audience.human_public_key}
                            </p>
                          ) : null}
                        </>
                      ) : record.audience && "channel_id" in record.audience ? (
                        <>
                          <p>
                            Audience:{" "}
                            {record.audience.kind === "thread"
                              ? "this thread"
                              : "this channel"}
                          </p>
                          <p className="break-all">
                            Channel: {record.audience.channel_id}
                          </p>
                          {record.audience.thread_root_event_id ? (
                            <p className="break-all">
                              Thread: {record.audience.thread_root_event_id}
                            </p>
                          ) : null}
                        </>
                      ) : null}
                    </div>
                  ) : null}
                  {scoped ? (
                    <p className="break-all text-xs text-muted-foreground">
                      Approval: {record.approval_id}
                    </p>
                  ) : null}
                  <p className="text-xs text-muted-foreground">
                    Use expires:{" "}
                    <time dateTime={record.expires_at}>
                      {new Date(record.expires_at).toLocaleString()}
                    </time>
                  </p>
                </li>
              ))}
            </ul>
            <p className="text-xs text-muted-foreground">
              {employee
                ? "Stop using employee or relationship facts is available in the employee’s reviewed memory controls; conversation and project facts keep their project controls."
                : conversation
                  ? "Stop using is available in the project’s conversation memory or reviewed project memory controls."
                  : "Stop using is available in the project’s reviewed memory controls."}{" "}
              Provider inputs already sent cannot be retracted.
            </p>
          </section>
        ) : null}
        {write ? (
          <Alert
            variant={write.status === "failed" ? "destructive" : "default"}
            role="status"
          >
            <AlertTitle>
              {write.status === "acknowledged"
                ? "Reply saved to memory"
                : write.status === "failed"
                  ? "Memory write failed"
                  : "Memory write pending"}
            </AlertTitle>
            <AlertDescription>
              <p>
                {write.withheld
                  ? employee
                    ? "This recorded write outcome is retained. Its note text is withheld because the requester or current memory permissions do not permit this view."
                    : "This recorded write outcome is retained. Its note text is withheld because this run’s conversation authority is no longer current."
                  : write.status === "acknowledged"
                    ? "Memory confirmed the notes from the Office reply."
                    : write.status === "failed"
                      ? "Automatic attempts have stopped. The run and its Office reply remain available."
                      : "Office accepted the reply; memory has not confirmed the write yet."}
              </p>
              <details className="mt-2">
                <summary className="cursor-pointer">
                  {write.withheld
                    ? "View write receipt and source"
                    : "View notes and source"}
                </summary>
                <div className="mt-2 flex flex-col gap-2">
                  {!write.withheld ? (
                    <NoteText content={write.content} />
                  ) : null}
                  <p className="break-all">Source: {write.source}</p>
                  {write.receipt ? (
                    <p>{write.receipt.written} note(s) confirmed</p>
                  ) : null}
                  {write.acknowledged_at ? (
                    <time dateTime={write.acknowledged_at}>
                      {new Date(write.acknowledged_at).toLocaleString()}
                    </time>
                  ) : null}
                </div>
              </details>
            </AlertDescription>
          </Alert>
        ) : (
          <p className="text-sm text-muted-foreground">
            No memory write has been scheduled.
          </p>
        )}
      </CardContent>
      <CardFooter>
        <p className="text-xs text-muted-foreground">
          {employee
            ? "Employee and relationship facts stay within their explicit shared destination and requester. Other facts keep their original audiences; scratch notes remain scoped to this run."
            : conversation
              ? "Conversation facts stay within their reviewed audiences; any project facts stay with their project. Scratch notes remain scoped to this run."
              : reviewed.length > 0
                ? "This run used explicitly approved facts for its own employee and project. Scratch notes remain scoped to this run."
                : "These notes stay with this run. They are not shared across employees or projects."}
        </p>
      </CardFooter>
    </Card>
  );
}
