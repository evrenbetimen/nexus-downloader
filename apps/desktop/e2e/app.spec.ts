import { expect, test, type Page } from "@playwright/test";

const rows = (page: Page) => page.locator('[role="row"][aria-selected]');

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(m.text());
  });
  (page as Page & { errors: string[] }).errors = errors;
  await page.goto("/");
  await expect(page.getByText("Simulated engine")).toBeVisible();
});

test.afterEach(async ({ page }) => {
  expect((page as Page & { errors: string[] }).errors).toEqual([]);
});

test("lists 10,000+ downloads with a virtualized grid", async ({ page }) => {
  await expect(page.getByRole("button", { name: /All Downloads/ })).toContainText("10,004");
  const rendered = await rows(page).count();
  expect(rendered).toBeGreaterThan(10);
  expect(rendered).toBeLessThan(80);

  // Scrolling to the bottom renders the last rows without mounting the rest.
  await page.getByRole("grid").evaluate((el) => el.scrollTo(0, el.scrollHeight));
  await expect(rows(page).last()).toBeVisible();
  expect(await rows(page).count()).toBeLessThan(80);
});

test("category sidebar filters rows", async ({ page }) => {
  await page.getByRole("button", { name: /^Videos/ }).click();
  const names = await rows(page).locator('[role="gridcell"]:first-child').allInnerTexts();
  expect(names.length).toBeGreaterThan(0);
  for (const name of names) expect(name).toMatch(/\.mkv$/);

  await page.getByRole("button", { name: /^Unfinished/ }).click();
  await expect(rows(page)).toHaveCount(4);
});

test("search narrows the list", async ({ page }) => {
  await page.getByPlaceholder("Filter by name").fill("lofi-set-vol3-10.");
  await expect(rows(page)).toHaveCount(1);
  await expect(rows(page).first()).toContainText("lofi-set-vol3-10.flac");
});

test("live rows show speed, ETA and connections", async ({ page }) => {
  const first = rows(page).first();
  await expect(first).toContainText("Downloading");
  await expect(first).toContainText(/\/s/);
  await expect(page.getByText(/^\d+ active/)).toContainText("4 active");
});

test("selecting a download shows its segment map", async ({ page }) => {
  await rows(page).first().click();
  await expect(page.getByRole("img", { name: "Segment map" })).toBeVisible();
  // Some cells have been painted (not just the empty background).
  await expect
    .poll(() =>
      page.getByRole("img", { name: "Segment map" }).evaluate((canvas: HTMLCanvasElement) => {
        const ctx = canvas.getContext("2d")!;
        const data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
        let lit = 0;
        for (let i = 0; i < data.length; i += 4) if (data[i + 2]! > 200) lit += 1;
        return lit;
      }),
    )
    .toBeGreaterThan(0);
});

test("pause and resume the selected download", async ({ page }) => {
  const first = rows(page).first();
  await first.click();
  const name = await first.locator('[role="gridcell"]').first().innerText();
  await page.getByRole("button", { name: "Pause" }).click();
  const row = rows(page).filter({ hasText: name });
  await expect(row).toContainText("Paused");
  await expect(page.getByRole("button", { name: "Pause" })).toBeDisabled();
  await page.getByRole("button", { name: "Resume" }).click();
  await expect(row).toContainText("Downloading");
});

test("delete removes the selected download", async ({ page }) => {
  await page.getByPlaceholder("Filter by name").fill("keynote-4k-hdr-5.mkv");
  await rows(page).first().click();
  await page.getByRole("button", { name: "Delete" }).click();
  await expect(rows(page)).toHaveCount(0);
  await expect(page.getByRole("button", { name: /All Downloads/ })).toContainText("10,003");
});

test("add a download from the dialog", async ({ page }) => {
  await page.getByRole("button", { name: "Add URL" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByLabel("Address")).toBeFocused();
  await expect(dialog.getByRole("button", { name: "Start download" })).toBeDisabled();
  await dialog.getByLabel("Address").fill("https://example.com/files/new-album.flac");
  await dialog.getByRole("button", { name: "Start download" }).click();
  await expect(dialog).toBeHidden();
  await expect(rows(page).first()).toContainText("new-album.flac");
  await expect(page.getByRole("button", { name: /All Downloads/ })).toContainText("10,005");
});

test("the add dialog rejects non-URLs and closes with Escape", async ({ page }) => {
  await page.getByRole("button", { name: "Add URL" }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("Address").fill("not a url");
  await dialog.getByRole("button", { name: "Start download" }).click();
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
});

test("dropping a link opens the add dialog prefilled", async ({ page }) => {
  await page.evaluate(() => {
    const data = new DataTransfer();
    data.setData("text/uri-list", "https://cdn.example.com/video.mp4");
    window.dispatchEvent(new DragEvent("drop", { dataTransfer: data, cancelable: true }));
  });
  await expect(page.getByRole("dialog").getByLabel("Address")).toHaveValue(
    "https://cdn.example.com/video.mp4",
  );
});

test("browser captures open one confirmation each, keeping the session", async ({ page }) => {
  await page.evaluate(() => {
    const capture = (window as unknown as { __nexusdlCapture(d: object): void }).__nexusdlCapture;
    capture({
      url: "https://cdn.example.com/dl?id=42",
      fileName: "season-finale.mkv",
      fileSize: 1_610_612_736,
      referrer: "https://videos.example.com/watch/42",
      cookies: "session=abc",
      userAgent: "TestBrowser/1.0",
    });
    capture({ url: "https://mirror.example.org/tools.zip" });
  });

  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("heading")).toHaveText("Download from browser");
  await expect(dialog).toContainText("Captured from videos.example.com · 1.50 GB · signed-in session");
  await expect(dialog.getByLabel("Address")).toHaveValue("https://cdn.example.com/dl?id=42");
  await expect(dialog.getByLabel("Save as (optional)")).toHaveValue("season-finale.mkv");
  await dialog.getByRole("button", { name: "Start download" }).click();
  await expect(rows(page).first()).toContainText("season-finale.mkv");
  const request = await page.evaluate(() => (window as unknown as { __nexusdlLastRequest: unknown }).__nexusdlLastRequest);
  expect(request).toEqual({
    url: "https://cdn.example.com/dl?id=42",
    fileName: "season-finale.mkv",
    context: { cookies: "session=abc", referrer: "https://videos.example.com/watch/42", userAgent: "TestBrowser/1.0" },
  });

  // The second capture follows; cancelling it adds nothing.
  await expect(dialog.getByLabel("Address")).toHaveValue("https://mirror.example.org/tools.zip");
  await expect(dialog).toContainText("Captured from your browser");
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByRole("button", { name: /All Downloads/ })).toContainText("10,005");
});

test("speed limiter caps and uncaps total speed", async ({ page }) => {
  const slider = page.getByRole("slider", { name: "Speed limit" });
  await expect(slider).toHaveValue("100");
  await slider.fill("40");
  await expect(page.getByText(/capped at/)).toBeVisible();
  const label = page.getByText(/capped at/);
  await expect(label).toContainText(/(KB|MB)\/s/);
  await slider.fill("100");
  await expect(page.getByText(/capped at/)).toBeHidden();
  await expect(page.getByText("Unlimited")).toBeVisible();
});

test("column headers sort the grid", async ({ page }) => {
  await page.getByRole("columnheader", { name: "Name" }).click();
  const names = await rows(page).locator('[role="gridcell"]:first-child').allInnerTexts();
  const sorted = [...names].sort((a, b) => a.localeCompare(b, undefined, { numeric: true }));
  expect(names).toEqual(sorted);
});
