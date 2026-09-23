async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.route("**/jsonv2", (route) => route.fulfill({ json: { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] } }));
  await page.setViewportSize({ width: 1440, height: 1050 });
  await page.addInitScript(() => {
    localStorage.clear();
    let callback = 0;
    const users = Array.from({ length: 36 }, (_, index) => ({
      username: index === 0 ? "fixture_account_with_a_long_name" : `fixture_${String(index).padStart(2, "0")}`,
      host: index === 0 ? "database-host-with-a-long-name.example.invalid" : "localhost",
      plugin: index === 0 ? "ed25519 OR mysql_native_password" : "mysql_native_password",
      locked: "N",
    }));
    const state = window.testMariaDBLayout = { calls: [], unknown: [], users, holdSave: false, finishSave: null };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: () => ++callback,
      invoke: async (command, args = {}) => {
        state.calls.push({ command, args: JSON.parse(JSON.stringify(args)) });
        switch (command) {
          case "fetch_latest_app_release": return { version: "0.4.1", tagName: "v0.4.1", htmlUrl: "https://github.com/zoxile/fxserver-installer/releases/tag/v0.4.1" };
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.4.1";
          case "plugin:event|listen": return args.handler;
          case "plugin:event|unlisten":
          case "append_app_log":
          case "initialize_health_workspace":
          case "save_database_login":
          case "clear_database_login":
          case "validate_mariadb_credentials": return;
          case "load_database_login": return { host: "localhost", port: 3306, username: "fixture-admin", password: "fixture-only", database: "game" };
          case "configure_live_bridge": return { workspaceId: "default", enabled: false, connected: false, snapshot: null };
          case "read_app_logs": return { path: "mock.log", entries: [] };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: false };
          case "get_mariadb_status": return { installed: true, running: true, version: "11.4.13", serviceName: "MariaDB", serviceDisplayName: "MariaDB", installPath: null };
          case "get_mariadb_package_info": return { latestVersion: "11.4.13", installedPackageVersion: "11.4.13", updateAvailable: false, error: null };
          case "list_mariadb_users": return structuredClone(state.users);
          case "list_mariadb_databases": return ["game", "database_with_a_long_name_for_layout_verification"];
          case "get_mariadb_user_access": return {
            username: args.username,
            host: args.host,
            grants: Array.from({ length: 12 }, (_, i) => `GRANT SELECT, INSERT, UPDATE ON database_with_a_long_name_for_layout_verification.table_${i} TO '${args.username}'@'${args.host}'`),
            schemaPrivileges: Array.from({ length: 12 }, () => ({ database: "database_with_a_long_name_for_layout_verification", privilege: "SELECT", grantable: "NO" })),
            tablePrivileges: Array.from({ length: 12 }, (_, i) => ({ database: "game", table: `table_with_a_long_name_for_layout_verification_${i}`, privilege: "SELECT", grantable: "NO" })),
          };
          case "save_mariadb_user":
          case "update_mariadb_user":
            if (state.holdSave) return new Promise((resolve) => { state.finishSave = resolve; });
            return;
          case "delete_mariadb_user":
            state.users = state.users.filter((user) => user.username !== args.username || user.host !== args.host);
            return;
          default: state.unknown.push(command); throw new Error(`Unmocked MariaDB layout command: ${command}`);
        }
      },
    };
  });

  const openPanel = async () => {
    await page.reload({ waitUntil: "domcontentloaded" });
    const nav = page.getByRole("navigation", { name: "Workspace navigation" });
    const parent = nav.getByTitle("MariaDB", { exact: true });
    if (await parent.getAttribute("aria-expanded") !== "true") await parent.click();
    await nav.getByTitle("Manage MariaDB", { exact: true }).click();
    await page.getByText("Credentials validated.", { exact: true }).waitFor();
    await page.getByRole("region", { name: "MariaDB accounts", exact: true }).waitFor();
  };
  const addForm = page.getByRole("form", { name: "Add database user", exact: true });
  const editForm = page.getByRole("form", { name: "Edit database user", exact: true });
  const accountList = page.getByRole("region", { name: "MariaDB accounts", exact: true });
  const checkbox = (form) => form.getByRole("checkbox", { name: "Use mysql_native_password authentication (optional)", exact: true });
  const firstAccount = "fixture_account_with_a_long_name@database-host-with-a-long-name.example.invalid";
  const assert = (condition, message) => { if (!condition) throw new Error(message); };

  const checkForm = async (form) => {
    const toggle = checkbox(form);
    const problem = await form.evaluate((element) => {
      const card = element.closest('[data-slot="card"]');
      const bounds = card.getBoundingClientRect();
      const toggle = element.querySelector('[role="checkbox"]');
      const description = document.getElementById(toggle.getAttribute("aria-describedby"));
      if (!description || !element.contains(description)) return "Authentication help is not associated with the checkbox inside its form";
      if (!description.textContent.includes("preserve existing authentication") || !description.textContent.includes("connector requires it")) return "Default-preserving or connector-only help is missing";
      for (const child of [element, description, ...element.querySelectorAll('input:not([type="hidden"]), button, label')]) {
        if (!child.getClientRects().length) continue;
        const rect = child.getBoundingClientRect();
        if (rect.left < bounds.left - 1 || rect.right > bounds.right + 1 || rect.top < bounds.top - 1 || rect.bottom > bounds.bottom + 1) return `${child.tagName} escapes the user card`;
        if (child.textContent.trim() && child.scrollWidth > child.clientWidth + 1 && getComputedStyle(child).overflowX === "visible") return `${child.tagName} text overflows: ${child.textContent.trim()} (${child.scrollWidth}/${child.clientWidth})`;
      }
      const label = element.querySelector(`label[for="${CSS.escape(toggle.id)}"]`);
      if (!label || toggle.getBoundingClientRect().right > label.getBoundingClientRect().left) return "Checkbox overlaps its label";
      if (description.getBoundingClientRect().top < label.getBoundingClientRect().bottom) return "Authentication help overlaps its label";
      const submit = element.querySelector('button[type="submit"]');
      if (description.getBoundingClientRect().bottom > submit.getBoundingClientRect().top) return "Authentication help overlaps the save action";
      return null;
    });
    assert(!problem, problem);
    await toggle.scrollIntoViewIfNeeded();
    await toggle.click({ trial: true });
  };

  const checkLayout = async (width, editing) => {
    await page.setViewportSize({ width, height: 1050 });
    // Wait for the sidebar's width transition before measuring the content grid.
    await page.waitForFunction(() => !document.getAnimations().some((animation) => animation.playState === "running" && animation.effect?.getKeyframes().some((frame) => "width" in frame)));
    await checkForm(addForm);
    if (editing) await checkForm(editForm);
    const geometry = await accountList.evaluate((list, editing) => {
      const content = list.closest('[data-slot="card-content"]');
      const card = list.closest('[data-slot="card"]');
      const rect = list.getBoundingClientRect();
      const cardBottom = card.getBoundingClientRect().bottom - parseFloat(getComputedStyle(card).paddingBottom);
      const nextPane = content.querySelector("section, form");
      const cards = [...card.parentElement.parentElement.querySelectorAll('[data-slot="card"]')];
      const overlap = cards.some((a, index) => cards.slice(index + 1).some((b) => {
        const x = a.getBoundingClientRect();
        const y = b.getBoundingClientRect();
        return Math.min(x.right, y.right) - Math.max(x.left, y.left) > 1 && Math.min(x.bottom, y.bottom) - Math.max(x.top, y.top) > 1;
      }));
      return {
        height: Math.round(rect.height),
        fills: editing || Math.abs(cardBottom - rect.bottom) < 2,
        bounded: list.scrollHeight > list.clientHeight && list.clientHeight >= 280,
        clipped: list.scrollWidth > list.clientWidth + 1 || cards.some((card) => card.scrollWidth > card.clientWidth + 1 || card.getBoundingClientRect().right > innerWidth + 1),
        overlap: overlap || Boolean(nextPane && rect.bottom > nextPane.getBoundingClientRect().top + 1),
      };
    }, editing);
    assert(geometry.fills, `${width}px: list leaves unused card height`);
    assert(geometry.bounded, `${width}px: populated list is not bounded and scrollable`);
    assert(!geometry.clipped && !geometry.overlap, `${width}px: cards overlap or overflow: ${JSON.stringify(geometry)}`);
    await accountList.focus();
    await accountList.press("End");
    await page.waitForFunction(() => {
      const list = document.querySelector('[aria-label="MariaDB accounts"]');
      return list.scrollTop > 0 && Math.abs(list.scrollHeight - list.clientHeight - list.scrollTop) < 2;
    });
    const lastEdit = accountList.getByRole("button", { name: "Edit fixture_35@localhost", exact: true });
    await lastEdit.focus();
    await lastEdit.click({ trial: true });
    await accountList.getByRole("button", { name: "Delete fixture_35@localhost", exact: true }).click({ trial: true });
    await accountList.focus();
    await accountList.getByRole("button", { name: `Edit ${firstAccount}`, exact: true }).scrollIntoViewIfNeeded();
    await accountList.evaluate((list) => { list.scrollTo({ top: 0, behavior: "instant" }); list.scrollIntoView({ block: "center", behavior: "instant" }); });
    await page.waitForFunction(() => document.querySelector('[aria-label="MariaDB accounts"]').scrollTop === 0);
    await page.screenshot({ path: `output/playwright/mariadb-layout-${width}-${editing ? "editing-list" : "users"}.png`, fullPage: true });
    const form = editing ? editForm : addForm;
    await form.evaluate((form) => form.scrollIntoView({ block: "center", behavior: "instant" }));
    await page.screenshot({ path: `output/playwright/mariadb-layout-${width}-${editing ? "edit" : "add"}-form.png`, fullPage: true });
    console.log(`Layout ${width}px ${editing ? "editing" : "populated"}: list ${geometry.height}px, bounded scrolling, no card/control overlaps`);
  };

  await openPanel();
  assert(!await checkbox(addForm).isChecked(), "New-user authentication must default to unchanged");
  for (const width of [900, 1100, 1440]) await checkLayout(width, false);
  await accountList.getByRole("button", { name: `Edit ${firstAccount}`, exact: true }).click();
  await editForm.waitFor();
  await page.getByRole("region", { name: "Selected account access", exact: true }).getByText("Table Access", { exact: true }).waitFor();
  assert(!await checkbox(editForm).isChecked(), "Editing authentication must default to unchanged");
  for (const width of [900, 1100, 1440]) await checkLayout(width, true);
  assert(await page.getByText(/Game-user compatibility/).count() === 0, "Parent integration pending: remove the old external compatibility checkbox");

  const callCount = (command) => page.evaluate((command) => window.testMariaDBLayout.calls.filter((call) => call.command === command).length, command);
  const lastConfig = (command) => page.evaluate((command) => window.testMariaDBLayout.calls.filter((call) => call.command === command).at(-1)?.args.config, command);
  const waitForCall = (command, previous) => page.waitForFunction(({ command, previous }) => window.testMariaDBLayout.calls.filter((call) => call.command === command).length > previous, { command, previous });
  await addForm.getByTitle("Database username to create or update.", { exact: true }).fill("fixture-new");
  await addForm.getByPlaceholder("User password", { exact: true }).fill("fixture-only");
  await addForm.getByRole("button", { name: "Add User", exact: true }).click();
  await waitForCall("save_mariadb_user", 0);
  assert((await lastConfig("save_mariadb_user")).nativePassword === false, "Default add changed authentication");
  await checkbox(addForm).focus();
  await checkbox(addForm).press("Space");
  assert(await checkbox(addForm).isChecked(), "Authentication checkbox is not keyboard operable");
  assert(!await checkbox(editForm).isChecked(), "Add-user and edit-user authentication leaked into each other");
  await page.evaluate(() => { window.testMariaDBLayout.holdSave = true; });
  await addForm.getByRole("button", { name: "Add User", exact: true }).click();
  await page.waitForFunction(() => Boolean(window.testMariaDBLayout.finishSave));
  assert((await lastConfig("save_mariadb_user")).nativePassword === true, "Parent must pass bind:nativePassword to UserManagementCard");
  assert(await checkbox(addForm).isDisabled() && await checkbox(editForm).isDisabled(), "Authentication controls stayed enabled while busy");
  await page.evaluate(() => { window.testMariaDBLayout.holdSave = false; window.testMariaDBLayout.finishSave(); });
  await checkbox(addForm).uncheck();
  await editForm.getByRole("button", { name: "Save Changes", exact: true }).click();
  await waitForCall("update_mariadb_user", 0);
  assert((await lastConfig("update_mariadb_user")).nativePassword === false, "Default edit changed authentication");
  await checkbox(editForm).check();
  await editForm.getByPlaceholder("New password or blank", { exact: true }).fill("fixture-replacement");
  await editForm.getByRole("button", { name: "Save Changes", exact: true }).click();
  await waitForCall("update_mariadb_user", 1);
  assert((await lastConfig("update_mariadb_user")).nativePassword === true, "Parent must pass bind:nativePassword={editNativePassword} to ExistingUsersCard");
  await accountList.getByRole("button", { name: "Edit fixture_01@localhost", exact: true }).click();
  assert(!await checkbox(editForm).isChecked(), "Authentication opt-in survived switching accounts");
  const deletes = await callCount("delete_mariadb_user");
  await accountList.getByRole("button", { name: "Delete fixture_01@localhost", exact: true }).click();
  await waitForCall("delete_mariadb_user", deletes);
  await editForm.waitFor({ state: "hidden" });

  await page.evaluate(() => { window.testMariaDBLayout.users = []; });
  await page.getByRole("button", { name: "Refresh existing MariaDB users", exact: true }).click();
  await page.getByText("No users loaded. Refresh to fetch accounts from MariaDB.", { exact: true }).waitFor();
  await page.getByTitle("Admin username used to connect to MariaDB.", { exact: true }).fill("not-applied");
  assert(await checkbox(addForm).isDisabled(), "Authentication control is enabled without validated credentials");
  const unknown = await page.evaluate(() => window.testMariaDBLayout.unknown);
  assert(!unknown.length && !errors.length, [...unknown, ...errors].join("\n"));
  console.log("PASS: MariaDB cards at 900/1100/1440px, 36 users, long grants/names, edit/access panes, bounded keyboard scrolling, internal checkboxes, independent parent bindings, default preservation, busy/credential guards, account actions and empty state. All database calls mocked.");
}
