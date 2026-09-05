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
          {memory.recall.status === "not_prepared" ? (
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
                {write.status === "acknowledged"
                  ? "Memory confirmed the notes from the Office reply."
                  : write.status === "failed"
                    ? "Automatic attempts have stopped. The run and its Office reply remain available."
                    : "Office accepted the reply; memory has not confirmed the write yet."}
              </p>
              <details className="mt-2">
                <summary className="cursor-pointer">
                  View notes and source
                </summary>
                <div className="mt-2 flex flex-col gap-2">
                  <NoteText content={write.content} />
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
          These notes stay with this run. They are not shared across employees
          or projects.
        </p>
      </CardFooter>
    </Card>
  );
}
