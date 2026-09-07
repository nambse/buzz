import { createHash } from "node:crypto";
import { expect, test } from "@playwright/test";
import { waitForAnimations } from "../../../../tests/helpers/animations.ts";
import { installMockBridge } from "../../../../tests/helpers/bridge.ts";

// Fixtures replace only HTTP responses; the screen, forms and native signing
// seam are production paths. No live project, employee or runtime is mutated.
test("manual work retains uncertain writes across tabs, resolves review, and clears revoked content", async ({
  page,
}, testInfo) => {
  // Keep the bounded review panel inside the scroll viewport for complete captures.
  await page.setViewportSize({ width: 1280, height: 1400 });
  await installMockBridge(page);
  const project = {
    id: "project-one",
    name: "Release planning",
    slug: "release",
    description: "Manual release plan",
    status: "active",
    version: 1,
    channel_id: "planning-channel",
    role: "owner",
    can_contribute: true,
    can_review: true,
  };
  let item = null;
  const writes = [];
  let uncertain = true;
  let revoked = false;
  await page.route("http://127.0.0.1:3010/api/v1/**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const signed = JSON.parse(
      Buffer.from(
        request.headers().authorization.slice(6),
        "base64",
      ).toString(),
    );
    const tags = Object.fromEntries(signed.tags);
    expect(tags.u).toBe(request.url());
    expect(tags.method).toBe(request.method());
    let body;
    let status = 200;
    if (
      url.pathname.startsWith("/api/v1/projects") ||
      url.pathname.startsWith("/api/v1/work-items")
    ) {
      if (revoked) {
        status = 403;
        body = {};
      } else if (request.method() === "POST") {
        const raw = request.postData();
        const input = JSON.parse(raw);
        expect(tags.payload).toBe(
          createHash("sha256").update(raw).digest("hex"),
        );
        expect(input.operation_id).toMatch(/^[a-f0-9-]{36}$/);
        writes.push({ path: url.pathname, raw, nonce: tags.nonce });
        if (url.pathname.endsWith("/work-items")) {
          item ??= {
            id: "work-one",
            project_id: project.id,
            title: input.title,
            description: input.description,
            priority: input.priority,
            state: "review",
            version: 1,
            criteria: [
              {
                id: "criterion-one",
                position: 0,
                text: input.criteria[0],
                status: "pending",
              },
            ],
            approvals: [
              {
                id: "approval-one",
                gate: "review",
                required: true,
                status: "pending",
                reason: null,
              },
            ],
            assignments: [],
            history: [],
            history_omitted: false,
            history_truncated: false,
            execution_available: false,
          };
          if (uncertain) {
            uncertain = false;
            status = 503;
          }
        } else if (url.pathname.endsWith("/satisfy")) {
          expect(input.expected_version).toBe(item.version);
          item.criteria[0].status = "satisfied";
          item.version++;
        } else if (url.pathname.endsWith("/resolve")) {
          expect(input.expected_version).toBe(item.version);
          expect(input.decision).toBe("approve");
          item.approvals[0].status = "approved";
          item.version++;
        } else if (url.pathname.endsWith("/transitions")) {
          expect(input.expected_version).toBe(item.version);
          expect(input.target).toBe("completed");
          item.state = "completed";
          item.version++;
        }
        body = { work_item: item };
      } else if (url.pathname === "/api/v1/projects")
        body = {
          projects: [project],
          next_cursor: null,
          can_create_projects: true,
          create_channels: [{ id: "planning-channel", name: "Planning" }],
        };
      else if (url.pathname.endsWith("/work-items"))
        body = { work_items: item ? [item] : [], next_cursor: null };
      else if (url.pathname.startsWith("/api/v1/work-items/"))
        body = { work_item: item };
      else body = { project };
    } else if (url.pathname === "/api/v1/employees")
      body = { employees: [], has_more: false, next_after: null };
    else body = { runs: [], has_more: false, next_cursor: null };
    await route.fulfill({
      status,
      contentType: "application/json",
      body: JSON.stringify(body),
      headers: { "Access-Control-Allow-Origin": "http://127.0.0.1:4177" },
    });
  });
  await page.goto("/");
  await page.getByTestId("open-agents-view").click();
  await page.getByRole("tab", { name: "Projects & Work" }).click();
  await expect(
    page.getByRole("button", { name: /Release planning/ }),
  ).toBeVisible();
  await waitForAnimations(page);
  await page
    .getByRole("region", { name: "Projects and manual work" })
    .screenshot({ path: testInfo.outputPath("projects.png") });
  await page.getByRole("button", { name: /Release planning/ }).click();
  await page.getByText("New work item", { exact: true }).click();
  await page
    .getByLabel("Work title", { exact: true })
    .fill("Review release notes");
  await page
    .getByLabel("Acceptance criteria (one per line)")
    .fill("Check the saved receipt");
  await page.getByLabel("Require reviewer approval before completion").check();
  await page
    .getByRole("button", { name: "Create work item", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Retry same operation" }),
  ).toBeVisible();
  expect(writes).toHaveLength(1);
  await page.getByRole("tab", { name: "Employees", exact: true }).click();
  await page.getByRole("tab", { name: "Projects & Work" }).click();
  await page.getByRole("button", { name: "Retry same operation" }).click();
  await expect(
    page.getByText("Work item saved.", { exact: true }),
  ).toBeVisible();
  expect(writes).toHaveLength(2);
  expect(writes[1].raw).toBe(writes[0].raw);
  expect(writes[1].nonce).not.toBe(writes[0].nonce);
  await page.getByRole("button", { name: /Review release notes/ }).click();
  const detail = page.getByRole("region", { name: "Work item detail" });
  await expect(
    detail.getByRole("heading", { name: "Review release notes" }),
  ).toBeFocused();
  await expect(
    detail.getByText(/This does not start or confirm employee execution/),
  ).toBeVisible();
  await waitForAnimations(page);
  await detail.screenshot({ path: testInfo.outputPath("work-review.png") });
  await detail
    .getByRole("button", { name: "Accept criterion: Check the saved receipt" })
    .click();
  await expect(
    detail.getByText("Check the saved receipt · satisfied"),
  ).toBeVisible();
  await detail
    .getByLabel("Decision for review", { exact: true })
    .selectOption("approve");
  await detail.getByRole("button", { name: "Save approval" }).click();
  await expect(detail.getByText("review · required · approved")).toBeVisible();
  await detail.getByLabel("New manual status").selectOption("completed");
  await detail.getByRole("button", { name: "Save status" }).click();
  await expect(detail.getByText("completed", { exact: true })).toBeVisible();
  await expect(detail.getByRole("button", { name: "Save status" })).toHaveCount(
    0,
  );
  await waitForAnimations(page);
  await detail.screenshot({ path: testInfo.outputPath("work-completed.png") });
  revoked = true;
  await page.getByRole("button", { name: "Refresh work" }).click();
  await expect(
    page.getByText("Your account does not have permission for this action."),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Review release notes" }),
  ).toHaveCount(0);
});
