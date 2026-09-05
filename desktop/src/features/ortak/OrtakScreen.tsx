import { useEffect, useMemo, useState } from "react";
import { signRelayEvent } from "@/shared/api/tauri";
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
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/shared/ui/tabs";
import { createOrtakClient, OrtakApiError } from "./client";
import { WorkScreen } from "./work/WorkScreen";
import { RunPanel } from "./RunPanel";
import type { EmployeePage, RunPage } from "./types";

export function OrtakScreen({ origin }: { origin: string }) {
  const client = useMemo(
    () => createOrtakClient(origin, signRelayEvent),
    [origin],
  );
  const [employees, setEmployees] = useState<EmployeePage | null>(null);
  const [runs, setRuns] = useState<RunPage | null>(null);
  const [employeeAfter, setEmployeeAfter] = useState<string | undefined>();
  const [runCursor, setRunCursor] = useState<string | undefined>();
  const [selectedRun, setSelectedRun] = useState<string | null>(null);
  const [tab, setTab] = useState("employees");
  const [workOpened, setWorkOpened] = useState(false);
  const [accessRevoked, setAccessRevoked] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    // Refresh intentionally retries this page after the automatic retry budget.
    void refresh;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let failures = 0;
    setEmployees(null);
    setRuns(null);
    setError(null);
    async function poll() {
      const round = new AbortController();
      const signal = AbortSignal.any([controller.signal, round.signal]);
      try {
        const [directory, activity] = await Promise.all([
          client.employees(signal, employeeAfter),
          client.runs(signal, runCursor),
        ]);
        if (controller.signal.aborted) return;
        setAccessRevoked(false);
        setEmployees(directory);
        setRuns(activity);
        setError(null);
        failures = 0;
        timer = setTimeout(() => void poll(), 5000);
      } catch (cause) {
        round.abort();
        if (controller.signal.aborted) return;
        setError(
          cause instanceof Error
            ? cause.message
            : "Ortak could not load employees.",
        );
        const revoked =
          cause instanceof OrtakApiError &&
          [401, 403, 404].includes(cause.status);
        if (revoked) {
          setAccessRevoked(true);
          setEmployees(null);
          setRuns(null);
          setSelectedRun(null);
        }
        failures += 1;
        if (!revoked && failures < 5)
          timer = setTimeout(
            () => void poll(),
            Math.min(3000 * 2 ** (failures - 1), 30_000),
          );
      }
    }
    void poll();
    return () => {
      controller.abort();
      if (timer) clearTimeout(timer);
    };
  }, [client, employeeAfter, runCursor, refresh]);
  const selected = runs?.runs.find((run) => run.run_id === selectedRun);
  const name =
    employees?.employees.find(
      (employee) => employee.employee_id === selected?.employee_id,
    )?.name ??
    selected?.employee_id ??
    "Employee";
  return (
    <main
      className="flex h-full min-h-0 flex-col gap-5 overflow-auto p-6"
      data-testid="ortak-employees"
    >
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">Employees</h1>
          <p className="text-sm text-muted-foreground">
            Your company’s employees and their recorded work.
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => setRefresh((value) => value + 1)}
        >
          Refresh
        </Button>
      </header>
      {error ? (
        <Alert variant="destructive">
          <AlertTitle>Could not refresh Ortak</AlertTitle>
          <AlertDescription>{error} Use Refresh to reconnect.</AlertDescription>
        </Alert>
      ) : null}
      <Tabs
        value={tab}
        onValueChange={(value) => {
          setTab(value);
          if (value === "work") setWorkOpened(true);
        }}
      >
        <TabsList aria-label="Ortak views">
          <TabsTrigger value="employees">Employees</TabsTrigger>
          <TabsTrigger value="activity">Activity</TabsTrigger>
          <TabsTrigger value="work">Projects &amp; Work</TabsTrigger>
        </TabsList>
        <TabsContent value="employees" className="flex flex-col gap-4">
          {!employees && !error ? <Skeleton className="h-40 w-full" /> : null}
          <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
            {employees?.employees.map((employee) => {
              const current = runs?.runs.find(
                (run) =>
                  run.employee_id === employee.employee_id &&
                  ["queued", "running", "waiting"].includes(run.status),
              );
              return (
                <Card key={employee.employee_id}>
                  <CardHeader>
                    <CardTitle>
                      {employee.name ?? employee.employee_id}
                    </CardTitle>
                    <CardDescription>
                      {employee.title ?? "Employee"}
                    </CardDescription>
                  </CardHeader>
                  <CardContent className="flex flex-col gap-3">
                    <div>
                      <Badge variant="secondary">{employee.status}</Badge>
                    </div>
                    <p className="text-sm text-muted-foreground">
                      {current
                        ? `Current visible run: ${current.status}`
                        : "No active run in this activity page."}
                    </p>
                    <p className="text-xs text-muted-foreground">
                      Saved status does not confirm runtime health.
                    </p>
                  </CardContent>
                  <CardFooter>
                    {current ? (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => {
                          setSelectedRun(current.run_id);
                          setTab("activity");
                        }}
                      >
                        View run
                      </Button>
                    ) : (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => setTab("activity")}
                      >
                        View activity
                      </Button>
                    )}
                  </CardFooter>
                </Card>
              );
            })}
          </div>
          {employees?.employees.length === 0 ? (
            <Alert role="status">
              <AlertDescription>
                No employees are available to this account.
              </AlertDescription>
            </Alert>
          ) : null}
          <div className="flex gap-2">
            {employeeAfter ? (
              <Button
                variant="outline"
                size="sm"
                onClick={() => setEmployeeAfter(undefined)}
              >
                First employees
              </Button>
            ) : null}
            {employees?.has_more && employees.next_after ? (
              <Button
                variant="outline"
                size="sm"
                onClick={() =>
                  setEmployeeAfter(employees.next_after ?? undefined)
                }
              >
                More employees
              </Button>
            ) : null}
          </div>
        </TabsContent>
        <TabsContent value="activity" className="flex flex-col gap-4">
          <div className="grid items-start gap-4 lg:grid-cols-[minmax(14rem,1fr)_minmax(0,2fr)]">
            <section className="flex flex-col gap-3" aria-label="Recorded runs">
              {!runs && !error ? <Skeleton className="h-40 w-full" /> : null}
              {runs?.runs.map((run) => (
                <Button
                  key={run.run_id}
                  variant={run.run_id === selectedRun ? "secondary" : "outline"}
                  className="h-auto justify-start gap-3 py-3"
                  onClick={() => setSelectedRun(run.run_id)}
                  aria-pressed={run.run_id === selectedRun}
                >
                  <span className="flex min-w-0 flex-col items-start gap-1">
                    <span>
                      {employees?.employees.find(
                        (employee) => employee.employee_id === run.employee_id,
                      )?.name ?? run.employee_id}
                    </span>
                    <span>
                      {run.status} ·{" "}
                      {new Date(run.timing.queued_at).toLocaleTimeString()}
                    </span>
                  </span>
                </Button>
              ))}
              {runs?.runs.length === 0 ? (
                <Alert role="status">
                  <AlertDescription>
                    No runs are available in this view.
                  </AlertDescription>
                </Alert>
              ) : null}
              <div className="flex flex-wrap gap-2">
                {runCursor ? (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => setRunCursor(undefined)}
                  >
                    Latest runs
                  </Button>
                ) : null}
                {runs?.has_more && runs.next_cursor ? (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => setRunCursor(runs.next_cursor ?? undefined)}
                  >
                    Older runs
                  </Button>
                ) : null}
              </div>
            </section>
            {selectedRun ? (
              <RunPanel
                key={`${origin}:${selectedRun}`}
                client={client}
                runId={selectedRun}
                employeeName={name}
              />
            ) : (
              <Alert role="status">
                <AlertTitle>Select a run</AlertTitle>
                <AlertDescription>
                  Inspect confirmed events and request cancellation when your
                  account permits it.
                </AlertDescription>
              </Alert>
            )}
          </div>
        </TabsContent>
        {workOpened ? (
          <TabsContent
            value="work"
            forceMount
            className={tab === "work" ? "" : "hidden"}
          >
            <WorkScreen
              key={origin}
              client={client}
              employees={employees?.employees ?? []}
              accessRevoked={accessRevoked}
            />
          </TabsContent>
        ) : null}
      </Tabs>
    </main>
  );
}
