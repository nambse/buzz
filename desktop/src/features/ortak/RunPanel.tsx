import { useEffect, useRef, useState } from "react";
import { Alert, AlertDescription, AlertTitle } from "@/shared/ui/alert";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/shared/ui/card";
import { Skeleton } from "@/shared/ui/skeleton";
import { describeActivity, DISPLAY_EVENT_LIMIT } from "./activity";
import type { OrtakClient } from "./client";
import type { Cancellation } from "./types";
import { RunMemoryPanel } from "./RunMemoryPanel";
import { useRunActivity } from "./useActivity";

export function RunPanel({
  client,
  runId,
  employeeName,
}: {
  client: OrtakClient;
  runId: string;
  employeeName: string;
}) {
  const [refresh, setRefresh] = useState(0);
  const [request, setRequest] = useState<Cancellation | null>(null);
  const [requestError, setRequestError] = useState<string | null>(null);
  const [requesting, setRequesting] = useState(false);
  const cancellationController = useRef<AbortController | null>(null);
  const { detail, entries, error, connected } = useRunActivity(
    client,
    runId,
    refresh,
  );
  useEffect(() => () => cancellationController.current?.abort(), []);
  const cancellation = detail?.cancellation ?? request;
  async function cancel() {
    const controller = new AbortController();
    cancellationController.current = controller;
    setRequesting(true);
    setRequestError(null);
    try {
      const result = await client.cancel(runId, controller.signal);
      if (!controller.signal.aborted) {
        setRequest(result);
        setRefresh((value) => value + 1);
      }
    } catch (cause) {
      if (!controller.signal.aborted)
        setRequestError(
          cause instanceof Error
            ? cause.message
            : "Cancellation could not be requested.",
        );
    } finally {
      if (!controller.signal.aborted) setRequesting(false);
    }
  }
  return (
    <section
      className="flex min-w-0 flex-col gap-4"
      aria-label={`${employeeName} run activity`}
    >
      <Card>
        <CardHeader>
          <CardTitle>{employeeName}</CardTitle>
          <CardDescription>Run activity and execution status</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          {detail ? (
            <div className="flex flex-wrap items-center gap-2">
              <Badge
                variant={
                  detail.detail.run.status === "failed"
                    ? "destructive"
                    : "secondary"
                }
              >
                {detail.detail.run.status}
              </Badge>
              <span className="text-xs text-muted-foreground">
                Started{" "}
                {new Date(
                  detail.detail.run.timing.started_at ??
                    detail.detail.run.timing.queued_at,
                ).toLocaleString()}
              </span>
            </div>
          ) : !error ? (
            <Skeleton className="h-6 w-40" />
          ) : null}
          {detail?.detail.error_message ? (
            <Alert variant="destructive">
              <AlertTitle>Run failed</AlertTitle>
              <AlertDescription>{detail.detail.error_message}</AlertDescription>
            </Alert>
          ) : null}
          {detail?.detail.cancel_reason ? (
            <p className="text-sm">{detail.detail.cancel_reason}</p>
          ) : null}
          {detail?.detail.run.outcome.delivery_intent === "silent" ? (
            <p className="text-sm text-muted-foreground">
              This run finished without an Office reply.
            </p>
          ) : null}
          {detail?.office_delivery ? (
            <Alert
              variant={
                detail.office_delivery.status === "failed"
                  ? "destructive"
                  : "default"
              }
              role={
                detail.office_delivery.status === "failed" ? "alert" : "status"
              }
            >
              <AlertTitle>
                {detail.office_delivery.status === "pending"
                  ? "Office reply pending"
                  : detail.office_delivery.status === "failed"
                    ? "Office reply failed"
                    : "Office reply delivered"}
              </AlertTitle>
              <AlertDescription>
                {detail.office_delivery.status === "pending"
                  ? "The run has completed; Office has not confirmed the reply yet."
                  : detail.office_delivery.status === "failed"
                    ? "The reply could not be posted to Office. The completed run and its activity remain available."
                    : "Office accepted this run’s reply."}
                {detail.office_delivery.status === "delivered" &&
                detail.office_delivery.delivered_at ? (
                  <time
                    className="mt-1 block text-xs"
                    dateTime={detail.office_delivery.delivered_at}
                  >
                    {new Date(
                      detail.office_delivery.delivered_at,
                    ).toLocaleString()}
                  </time>
                ) : null}
              </AlertDescription>
            </Alert>
          ) : null}
          {cancellation ? (
            <Alert role="status">
              <AlertTitle>
                {cancellation.status === "pending"
                  ? "Cancellation requested"
                  : cancellation.status === "failed"
                    ? "Cancellation failed"
                    : "Cancellation acknowledged"}
              </AlertTitle>
              <AlertDescription>
                {cancellation.status === "pending"
                  ? "The worker has not confirmed that execution stopped."
                  : cancellation.status === "failed"
                    ? "The worker could not complete cancellation. The run status above remains authoritative."
                    : "The worker recorded its terminal acknowledgement."}
              </AlertDescription>
            </Alert>
          ) : null}
          {requestError ? (
            <Alert variant="destructive">
              <AlertTitle>Could not request cancellation</AlertTitle>
              <AlertDescription>{requestError}</AlertDescription>
            </Alert>
          ) : null}
        </CardContent>
        <CardFooter className="flex flex-wrap gap-2">
          {detail?.can_request_cancel && !cancellation ? (
            <Button
              variant="destructive"
              size="sm"
              disabled={requesting}
              onClick={() => void cancel()}
            >
              {requesting ? "Requesting cancellation…" : "Cancel run"}
            </Button>
          ) : null}
          <Button
            variant="outline"
            size="sm"
            onClick={() => setRefresh((value) => value + 1)}
          >
            Reload timeline
          </Button>
        </CardFooter>
      </Card>
      {detail?.memory ? <RunMemoryPanel memory={detail.memory} /> : null}
      {error ? (
        <Alert variant="destructive">
          <AlertTitle>Activity disconnected</AlertTitle>
          <AlertDescription>
            {error} Reload the timeline to try again.
          </AlertDescription>
        </Alert>
      ) : null}
      <p className="text-xs text-muted-foreground" role="status">
        {connected
          ? "Showing confirmed activity"
          : error
            ? "Updates paused"
            : "Connecting to activity…"}
        {entries.length === DISPLAY_EVENT_LIMIT
          ? ` · Latest ${DISPLAY_EVENT_LIMIT} events shown`
          : ""}
      </p>
      <ol className="flex flex-col gap-2" aria-label="Ordered run events">
        {entries.map((entry) => {
          const item = describeActivity(entry);
          return (
            <li
              key={entry.sequence}
              className="rounded-lg border border-border p-3"
            >
              <div className="flex flex-wrap items-center justify-between gap-2">
                <h3 className="text-sm font-medium">{item.title}</h3>
                <time
                  className="text-xs text-muted-foreground"
                  dateTime={entry.occurred_at}
                >
                  {new Date(entry.occurred_at).toLocaleTimeString()}
                </time>
              </div>
              {item.detail ? (
                <pre className="mt-2 whitespace-pre-wrap break-words font-mono text-xs">
                  {item.detail}
                </pre>
              ) : null}
              {entry.redacted || entry.truncated ? (
                <p className="mt-2 text-xs text-muted-foreground">
                  {entry.redacted ? "Sensitive text was redacted. " : ""}
                  {entry.truncated
                    ? "Output was shortened before storage."
                    : ""}
                </p>
              ) : null}
            </li>
          );
        })}
      </ol>
      {connected && entries.length === 0 ? (
        <Alert role="status">
          <AlertDescription>
            No activity has been recorded for this run yet.
          </AlertDescription>
        </Alert>
      ) : null}
    </section>
  );
}
