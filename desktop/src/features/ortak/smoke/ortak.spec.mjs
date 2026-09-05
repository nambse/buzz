import { expect, test } from "@playwright/test";
import { waitForAnimations } from "../../../../tests/helpers/animations.ts";
import { installMockBridge } from "../../../../tests/helpers/bridge.ts";

// Only test fixtures use fabricated records. The screen and native-signing
// bridge are production code; these responses isolate rendering from PostgreSQL.
test("Employees renders ordered Activity, cancellation, and separate Office delivery states", async ({
  page,
}) => {
  await installMockBridge(page);
  let requests = 0;
  let cancellation = null;
  let officeDelivery = null;
  const memory = {
    scope: "run_scratch",
    run_id: "test-run",
    recall: {
      status: "prepared",
      prepared_at: "2026-09-05T12:00:00Z",
      truncated: false,
      records: [
        {
          record_ref: "fixture-note",
          content: { text: "Keep the answer concise." },
          source: "run_note",
          recorded_at: "2026-09-05T11:59:00Z",
        },
      ],
    },
    write: null,
  };
  const run = {
    run_id: "test-run",
    employee_id: "test-employee",
    status: "running",
    outcome: { kind: "pending" },
    last_event: { sequence: 1 },
    timing: {
      queued_at: "2026-09-05T12:00:00Z",
      started_at: "2026-09-05T12:00:01Z",
    },
    provenance: {},
  };
  const entries = [
    {
      sequence: 0,
      event_type: "run.queued",
      occurred_at: "2026-09-05T12:00:00Z",
      activity: { kind: "lifecycle", phase: { phase: "queued" } },
    },
    {
      sequence: 1,
      event_type: "assistant.output",
      occurred_at: "2026-09-05T12:00:02Z",
      activity: {
        kind: "assistant_output",
        text: { text: "Validation complete." },
      },
    },
  ];
  await page.route("http://127.0.0.1:3010/api/v1/**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const signed = JSON.parse(
      Buffer.from(
        request.headers().authorization.slice(6),
        "base64",
      ).toString(),
    );
    expect(Object.fromEntries(signed.tags).u).toBe(request.url());
    expect(Object.fromEntries(signed.tags).method).toBe(request.method());
    let body;
    let status = 200;
    if (url.pathname.endsWith("/cancel")) {
      expect(request.postData()).toBe("{}");
      requests++;
      if (requests === 1) {
        status = 503;
        body = { error: { code: "unavailable" } };
      } else {
        status = 202;
        cancellation = {
          request_id: "request-1",
          run_id: run.run_id,
          status: "pending",
          requested_at: "2026-09-05T12:00:03Z",
        };
        body = cancellation;
      }
    } else if (url.pathname.endsWith("/events")) {
      const cursor = url.searchParams.get("after_sequence");
      body = {
        entries: cursor === null ? entries : [],
        next_after_sequence: 1,
        has_more: false,
        gap: null,
      };
    } else if (url.pathname.endsWith("/test-run")) {
      body = {
        detail: { run, error_message: null, cancel_reason: null },
        cancellation,
        office_delivery: officeDelivery,
        memory,
        can_request_cancel: cancellation === null,
      };
    } else if (url.pathname.endsWith("/employees")) {
      body = {
        employees: [
          {
            employee_id: "test-employee",
            name: "Test Employee",
            title: "Test role",
            status: "active",
            active_revision_id: "revision-1",
          },
        ],
        next_after: null,
        has_more: false,
      };
    } else body = { runs: [run], next_cursor: null, has_more: false };
    await route.fulfill({
      status,
      contentType: "application/json",
      body: JSON.stringify(body),
      headers: { "Access-Control-Allow-Origin": "http://127.0.0.1:4177" },
    });
  });
  await page.goto("/");
  await expect(page.getByTestId("open-agents-view")).toHaveAccessibleName(
    "Employees",
  );
  for (const id of [
    "open-projects-view",
    "open-workflows-view",
    "open-pulse-view",
  ])
    await expect(page.getByTestId(id)).toHaveCount(0);
  await page.getByTestId("open-search").click();
  await expect(page.getByTestId("search-results")).toBeVisible();
  await expect(
    page.getByText("Create a new agent", { exact: true }),
  ).toHaveCount(0);
  await page.keyboard.press("Escape");
  await page.evaluate(() =>
    window.dispatchEvent(
      new CustomEvent("buzz:open-create-agent", { detail: {} }),
    ),
  );
  await expect(page.getByRole("dialog")).toHaveCount(0);
  // A direct unavailable route must redirect before its production component mounts.
  await page.evaluate(() => {
    window.location.hash = "/projects/private-unbuilt";
  });
  await expect(page).toHaveURL(/\/agents$/);
  await page.getByTestId("open-agents-view").click();
  const screen = page.getByTestId("ortak-employees");
  await expect(
    screen.getByRole("heading", { name: "Employees", exact: true }),
  ).toBeVisible();
  await expect(
    screen.getByText("Test Employee", { exact: true }),
  ).toBeVisible();
  await screen.getByRole("button", { name: "View run", exact: true }).click();
  const timeline = screen.getByRole("list", { name: "Ordered run events" });
  await expect(timeline.locator("li")).toHaveCount(2);
  await expect(timeline.locator("li").nth(0)).toContainText("Run queued");
  await expect(timeline.locator("li").nth(1)).toContainText(
    "Validation complete.",
  );
  const cancel = screen.getByRole("button", {
    name: "Cancel run",
    exact: true,
  });
  await cancel.focus();
  await cancel.press("Enter");
  await expect(
    screen.getByText("Could not request cancellation", { exact: true }),
  ).toBeVisible();
  await cancel.click();
  await expect(
    screen.getByText("Cancellation requested", { exact: true }),
  ).toBeVisible();
  await expect(
    screen.getByText("The worker has not confirmed that execution stopped.", {
      exact: true,
    }),
  ).toBeVisible();
  await expect(cancel).toHaveCount(0);
  await expect(
    screen.getByRole("button", { name: "Reload timeline" }),
  ).toBeVisible();
  await waitForAnimations(page);
  await screen.screenshot({ path: "test-results/ortak-employees.png" });

  // A completed run is not a delivered Office reply. This must keep polling
  // even after cancellation is acknowledged and every activity event is read.
  run.status = "completed";
  run.outcome = { kind: "completed", delivery_intent: "reply" };
  cancellation.status = "acknowledged";
  officeDelivery = { status: "pending", error_code: null, delivered_at: null };
  await expect(
    screen.getByText("Office reply pending", { exact: true }),
  ).toBeVisible();
  await expect(screen.getByText("completed", { exact: true })).toBeVisible();
  await expect(
    screen.getByText("Office reply delivered", { exact: true }),
  ).toHaveCount(0);
  await waitForAnimations(page);
  await screen.screenshot({ path: "test-results/ortak-office-pending.png" });

  officeDelivery = {
    status: "failed",
    error_code: "office_rejected",
    delivered_at: null,
  };
  await expect(
    screen.getByText("Office reply failed", { exact: true }),
  ).toBeVisible();
  await expect(
    screen.getByText("Validation complete.", { exact: true }),
  ).toBeVisible();
  await waitForAnimations(page);
  await screen.screenshot({ path: "test-results/ortak-office-failed.png" });

  // Terminal delivery failure stops automatic polling; manual recovery remains.
  officeDelivery = {
    status: "delivered",
    error_code: null,
    delivered_at: "2026-09-05T12:00:04Z",
  };
  memory.write = {
    status: "pending",
    error_code: null,
    attempts: 1,
    next_attempt_at: "2026-09-05T12:00:05Z",
    content: { text: "Validation complete.", redacted: false },
    source: "office:fixture-signed-reply",
    recorded_at: "2026-09-05T12:00:04Z",
    receipt: null,
    acknowledged_at: null,
  };
  await screen.getByRole("button", { name: "Reload timeline" }).click();
  await expect(
    screen.getByText("Office reply delivered", { exact: true }),
  ).toBeVisible();
  await expect(
    screen.getByText("Office reply failed", { exact: true }),
  ).toHaveCount(0);
  await waitForAnimations(page);
  await screen.screenshot({ path: "test-results/ortak-office-delivered.png" });
  const memoryPanel = screen.locator('[aria-label="Run memory"]');
  await expect(
    memoryPanel.getByText("Memory write pending", { exact: true }),
  ).toBeVisible();
  await memoryPanel
    .getByText("View notes and source", { exact: true })
    .press("Enter");
  await expect(
    memoryPanel.getByText("Source: office:fixture-signed-reply", {
      exact: true,
    }),
  ).toBeVisible();
  await waitForAnimations(page);
  await memoryPanel.screenshot({
    path: "test-results/ortak-memory-pending.png",
  });
  memory.write.status = "acknowledged";
  memory.write.receipt = { reference: "fixture-receipt", written: 1 };
  memory.write.acknowledged_at = "2026-09-05T12:00:05Z";
  await expect(
    memoryPanel.getByText("Reply saved to memory", { exact: true }),
  ).toBeVisible();
  await expect(
    memoryPanel.getByText("1 note(s) confirmed", { exact: true }),
  ).toBeVisible();
  await expect(
    memoryPanel.getByText("Memory write pending", { exact: true }),
  ).toHaveCount(0);
  await waitForAnimations(page);
  await memoryPanel.screenshot({
    path: "test-results/ortak-memory-confirmed.png",
  });
});

test("private first-run setup keeps human identity and skips legacy runtime provisioning", async ({
  page,
}) => {
  await installMockBridge(page, undefined, {
    skipCommunitySeed: true,
    skipOnboardingSeed: true,
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Create a new identity key" }).click();
  await expect(page.getByTestId("onboarding-page-backup")).toBeVisible();
  await page.getByTestId("onboarding-next").click();
  await expect(
    page.getByRole("heading", { name: "Connect to your private Office" }),
  ).toBeVisible();
  await expect(page.getByTestId("ortak-onboarding-continue")).toBeVisible();
  await expect(page.getByTestId("onboarding-page-setup")).toHaveCount(0);
  await page.getByTestId("ortak-onboarding-continue").click();
  await expect(page.getByTestId("machine-onboarding-gate")).toHaveCount(0);
  const commands = await page.evaluate(
    () => window.__BUZZ_E2E_COMMANDS__ ?? [],
  );
  for (const command of [
    "create_managed_agent",
    "start_managed_agent",
    "reconcile_managed_agent_runtimes",
    "install_acp_runtime",
    "connect_acp_runtime",
  ])
    expect(commands).not.toContain(command);
});
