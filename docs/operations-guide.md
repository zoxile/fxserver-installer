# Reviewed Operations And Diagnostics

These tools extend the existing panels. They do not make a server or database safe to change without independent backups. Test changes on a disposable server first. Static checks cannot discover every runtime dependency or validate arbitrary SQL semantics.

## Slow Query Analysis

Open **Queries & Files**, expand **SQL Diagnostics**, and open **Slow query analyzer**. Import a local UTF-8 MariaDB slow-query `.log` up to 4 MiB. The app does not enable logging or alter MariaDB settings for you.

The analyzer groups normalized statements by database and query fingerprint. Sort by cumulative time, count, average time, maximum time, or rows examined. Time is measured in seconds and cumulative time is not wall-clock duration: queries can overlap.

Literal values and comments are removed. Identifiers may still be sensitive. Parsing runs in a worker and the report stays in memory until cleared or the panel closes. The maximum is 20,000 records and 2,000 groups; skipped or truncated records are reported rather than treated as a complete workload.

Index advice is advisory. Eligible simple SELECTs can produce an **EXPLAIN template** with placeholders for your review. The analyzer never executes imported SQL, changes indexes, or runs ANALYZE. Fill in appropriate values and review the database scope before using any template in the query console.

See the [MariaDB slow-query log documentation](https://mariadb.com/docs/server/server-management/server-monitoring-logs/slow-query-log) for logging setup and the meaning of its measurements.

## Saved Queries And Bookmarks

The query console has a saved-query library with named folders. Saved items are scoped to a workspace, host, port, and database username, not to a password. Changing a password does not create a new library partition.

Typed placeholders produce a draft for review, for example:

```sql
SELECT * FROM players WHERE citizenid = {{citizen_id:text}} LIMIT {{maximum:number}};
```

Supported values are `text`, `number`, `boolean`, and `null`. Values are scalar expressions, not SQL identifiers. Do not parameterize table names, column names, or SQL keywords. Preparing a draft is not execution and is not a server-side prepared statement. Review the exact SQL and destination before running it; confirmation does not turn arbitrary SQL into a safe query.

Database Browser bookmarks retain the selected table, view, filters, sorting, page size, and offset. Applying a bookmark still uses the current validated login and database permissions. It does not restore old row data or authorize writes.

Saved queries and bookmarks use bounded local storage. Do not store credentials, tokens, personal data, or secret filter values in them. The app rejects recognized secrets and known connection passwords, but no text detector can recognize every secret. Parameter values should remain temporary rather than being saved into SQL text. Invalid existing library data is not silently overwritten.

## Grouped Console Issues

Open **Server Logs** and expand the grouped console issues section. This uses the app-managed server console, separately from the existing raw txData log viewer.

Repeated warnings and errors are grouped by resource, severity, and normalized message. Search or filter the groups and inspect nearby context for a retained occurrence. Source line numbers and error codes remain meaningful; volatile durations and IDs can be normalized.

Counts, first/last occurrence, and context cover a bounded 1,000-line observed window. They are not all-time counts. Old occurrences expire, and workspace changes or cleared buffers reset the view. Polling runs only while the view is mounted and does not overlap requests. Raw logs remain available unchanged.

## Resource Dependency Inspection

Open **Resource Manager** and inspect dependencies, or review impact before a stop, restart, update, or reinstall. The inventory includes declared dependencies and providers from server-data and artifact resources, including supported resource junctions.

Direct and transitive dependents identify resources that may be affected. Missing dependencies, ambiguous providers, dynamic declarations, incomplete scans, and cycles are reported explicitly. Suggested startup order is based on declarations, not actual runtime behavior. The app does not execute manifest Lua to discover more relationships.

Confirming an action changes only the selected resource. It does not automatically stop or restart every dependent. Undeclared exports, events, external services, and remote RCON targets may differ from the inspected local inventory.

## Guided Profiler Recording

In **Profiler**, choose a bounded frame count and record the already-running local managed server. Save a working RCON password in **Manage Server** first. Recording does not start a server or install a bridge.

The app checks for an existing recording, captures through fixed profiler commands, saves to an app-generated temporary filename, and loads the result into the existing viewer. Progress and cancellation continue across panel navigation. Only bounded local capture files are accepted; a failure never falls back to an arbitrary file or a previous capture.

The local RCON endpoint must echo a random verification token into the managed server's own console before profiler commands are sent. A wrong port fails the check instead of recording another server. JSON analysis runs in a worker, with a 16 MiB input limit and a 15-second analysis deadline.

Cancellation never sends `profiler record stop`: FXServer has no recording identity, so that command could stop a replacement recording started outside the app. A started frame-bounded capture finishes as the server advances. Cancellation during export waits for the bounded export check. If writer completion cannot be confirmed, the app reports the retained temporary file and keeps it protected; cleanup is retried on the next capture request after the writer finishes or the managed server exits. No background cleanup is implied, and quitting the app can leave that reported file for manual cleanup after the server has stopped.

Keep two captures to compare timings before and after a change. Use similar workloads and inspect sample counts; a comparison is not proof that a resource update caused a performance change. Inclusive timings can overlap, and bridge scheduler delay is not per-resource CPU usage.

See the [Cfx.re profiler documentation](https://docs.fivem.net/docs/scripting-manual/debugging/using-profiler/) for capture concepts.

## Maintenance Plans

1. Start the selected server through **Manage Server**. Its profile and launch settings must match the workspace being reviewed.
2. Validate the database login and configure backup destinations under **Backups & Restore**. Select every database relevant to your planned changes; the app cannot infer arbitrary resource database usage.
3. Review each resource update and add it to the existing update queue. Protected files and update previews remain subject to the normal validation and expiry rules.
4. Open **Maintenance Plans** and choose backups. Optionally enable an RCON announcement and wait for an empty server using a connected Live Bridge.
5. Review the scope, accept downtime, and disable competing txAdmin restart schedules before confirming.

Execution verifies the reviewed server and backup settings, announces if requested, waits for players if requested, stops the managed server, backs up selected databases, applies reviewed updates, runs preflight, starts the server, and observes startup.

Backups run after shutdown to avoid writes from the managed server while they are taken. Other database clients can still write. Announcement delivery depends on your chat resource. Player waiting requires fresh bridge data and expires after ten minutes; it does not kick players.

The review is bound to the selected server settings, validated database login, backup destinations, and queued update previews. Changes require a new review. Restart uses the original launch snapshot held in native memory, including secret environment values; the snapshot is not written to disk or returned to the frontend. Announcements verify the local RCON endpoint against the managed process before sending.

The plan runs only while the app remains open, including the tray. Other app mutations are blocked during execution, but navigation and monitoring remain available. Stop requests take effect between operations; an in-flight backup or resource replacement is not interrupted halfway through.

Scheduled backups are deferred while maintenance holds its backup reservation. The app renews it every 30 seconds; it expires after two minutes without renewal. Lost reservations pause subsequent actions. The reservation does not interrupt an already-running backup and is released when the plan ends.

A failed step pauses the sequence. Completed backups and resource snapshots remain available. Inspect the step results and **Manage Server** before recovery: the server may be stopped, or may already have restarted if startup observation failed. No automatic database restore, SQL migration, resource rollback, or replay is performed. Resource rollback and database restore require separate review.

Without Live Bridge, startup observation confirms only that the managed process remained running for 15 seconds. With the bridge enabled, it must also reconnect within 90 seconds. Neither check guarantees every resource is healthy.

## Configuration Change Review

Saving a `.cfg` opens a review of the exact loaded version and draft. Checks cover literal includes, malformed commands, repeated settings, endpoint conflicts, and possible secret exposure. Normal TCP/UDP use of the same port is allowed. Changes to included configuration invalidate the validation review.

Secrets are hidden until explicitly revealed in the diff. Errors must be resolved; advisory warnings require acknowledgment for that save. Safe write boundaries and stale-file checks remain enforced independently of the review.

The validator is bounded and static. Dynamic includes and external linked configuration may require manual verification; the app does not execute configuration or follow arbitrary external paths to prove safety. External programs can still modify configuration, so stop competing editors during reviewed changes.
