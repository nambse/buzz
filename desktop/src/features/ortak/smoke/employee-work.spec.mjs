import { expect, test } from "@playwright/test";
import { waitForAnimations } from "../../../../tests/helpers/animations.ts";
import { installMockBridge } from "../../../../tests/helpers/bridge.ts";

test("employee queue is read-only, paginates, switches employees and clears revoked assignments", async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 1280, height: 1000 });
  await installMockBridge(page);
  let denied = false;
  const cursors = [];
  await page.route("http://127.0.0.1:3010/api/v1/**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    expect(request.method()).toBe("GET");
    const signed = JSON.parse(
      Buffer.from(
        request.headers().authorization.slice(6),
        "base64",
      ).toString(),
    );
    expect(Object.fromEntries(signed.tags).u).toBe(request.url());
    let body;
    let status = 200;
    if (url.pathname.endsWith("/work-items")) {
      const employee = url.pathname.split("/")[4];
      const cursor = url.searchParams.get("cursor");
      cursors.push({ employee, cursor });
      if (denied) {
        status = 403;
        body = {};
      } else
        body = {
          employee_id: employee,
          work_items:
            employee === "bea"
              ? []
              : [
                  {
                    id: cursor ? "second" : "first",
                    project_id: "project",
                    title: cursor
                      ? "Follow up with review"
                      : "Review release notes",
                    priority: "normal",
                    state: "review",
                    version: 2,
                    assignment_role: "reviewer",
                  },
                ],
          next_cursor: employee === "bea" || cursor ? null : "next+/=page",
          execution_available: false,
        };
    } else if (url.pathname === "/api/v1/employees")
      body = {
        employees: [
          {
            employee_id: "ada",
            name: "Ada",
            title: "Reviewer",
            status: "paused",
            active_revision_id: null,
          },
          {
            employee_id: "bea",
            name: "Bea",
            title: "Researcher",
            status: "draft",
            active_revision_id: null,
          },
        ],
        has_more: false,
        next_after: null,
      };
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
  await page
    .getByRole("button", { name: "View assigned work for Ada" })
    .click();
  const queue = page.getByRole("region", { name: "Employee assigned work" });
  await expect(
    queue.getByRole("heading", { name: "Ada’s assigned work" }),
  ).toBeFocused();
  await expect(
    queue.getByText(/Outstanding assignments remain visible while inactive/),
  ).toBeVisible();
  await expect(
    queue.getByRole("heading", { name: "Review release notes" }),
  ).toBeVisible();
  await expect(queue.getByText("Assignment role: reviewer")).toBeVisible();
  await expect(
    queue.getByText(/do not start or confirm employee execution/),
  ).toBeVisible();
  await waitForAnimations(page);
  await queue.screenshot({
    path: testInfo.outputPath("employee-assigned-work.png"),
  });
  await queue.getByRole("button", { name: "More assignments" }).click();
  await expect(
    queue.getByRole("heading", { name: "Follow up with review" }),
  ).toBeVisible();
  await expect(
    queue.getByRole("heading", { name: "Review release notes" }),
  ).toHaveCount(0);
  expect(cursors).toContainEqual({ employee: "ada", cursor: "next+/=page" });
  await page
    .getByRole("button", { name: "View assigned work for Bea" })
    .click();
  await expect(
    queue.getByRole("heading", { name: "Bea’s assigned work" }),
  ).toBeFocused();
  await expect(
    queue.getByText("No visible outstanding assignments in this page."),
  ).toBeVisible();
  await expect(
    queue.getByRole("heading", { name: "Follow up with review" }),
  ).toHaveCount(0);
  await page
    .getByRole("button", { name: "View assigned work for Ada" })
    .click();
  await expect(
    queue.getByRole("heading", { name: "Review release notes" }),
  ).toBeVisible();
  denied = true;
  await queue.getByRole("button", { name: "Refresh assigned work" }).click();
  await expect(queue.getByRole("alert")).toContainText("permission");
  await expect(
    queue.getByRole("heading", { name: "Review release notes" }),
  ).toHaveCount(0);
  await queue.getByRole("button", { name: "Close assigned work" }).click();
  await expect(
    page.getByRole("button", { name: "View assigned work for Ada" }),
  ).toBeFocused();
});
