import { invoke } from "@tauri-apps/api/core";
import { databaseSession, isDatabaseSessionValidated } from "$lib/core/databaseSession.svelte";
import { getInstallPath } from "$lib/core/paths.svelte";
import { taskSession, trackExclusiveTask } from "$lib/core/tasks.svelte";
import { workspaceSession } from "$lib/core/workspaces.svelte";
import { log } from "$lib/core/logger.svelte";
import { fxserverSettings, readSavedEnvironment } from "$lib/features/fxserver/fxserverSettings.svelte";
import { txHostFields } from "$lib/features/fxserver/fxserverEnv";
import type { BackupOverview, BackupSchedule, BackupSnapshot } from "./backupManager";
import type { FxserverLaunchRequest, FxserverLaunchResult, FxserverStatus } from "./fxserver";
import type { PreflightReport } from "./diagnostics";
import type { BridgeStatus } from "./liveBridge";
import type { ResourceSnapshot } from "./resourceUpdates";
import { getResourcePlan, getResourcePreference, loadResourcePreferences } from "./resourcePlan.svelte";
import { runReviewedUpdates, type ResourcePlan } from "./resourcePlan";
import { createMaintenanceRun, executeMaintenance, sameMaintenancePath, type MaintenanceRun } from "./maintenance";
import { reserveMaintenanceBackups } from "./maintenanceReservation";

export interface MaintenanceOptions { scheduleIds: string[]; announce: boolean; waitForPlayers: boolean }
export interface MaintenanceReview {
	id: string;
	launchToken: string;
	workspaceId: string;
	workspaceRevision: number;
	createdAt: number;
	launch: FxserverLaunchRequest;
	txDataPath: string;
	profile: string;
	pid: number;
	schedules: BackupSchedule[];
	updates: { id: string; name: string; files: number; protected: number }[];
	queueSignature: string;
	options: MaintenanceOptions;
	bridgeRequired: boolean;
	databaseTarget: { host: string; port: number; username: string };
}
export const maintenanceSession = $state({ workspaceId: "", review: null as MaintenanceReview | null, run: null as MaintenanceRun | null, snapshots: [] as BackupSnapshot[] });
const delay = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));
const queueSignature = (plan: ResourcePlan) => JSON.stringify(plan.entries.filter((entry) => entry.status === "ready").map((entry) => [entry.id, entry.target, entry.protectedPaths]));
let reviewedAuthentication: typeof databaseSession.validated = null;
let preparing = false;
let preparationGeneration = 0;

type MaintenanceScope = Pick<MaintenanceReview, "workspaceId" | "workspaceRevision" | "launch" | "txDataPath" | "profile">;

function currentReview(review: MaintenanceScope) {
	if (review.workspaceId !== workspaceSession.activeId || review.workspaceRevision !== workspaceSession.revision || taskSession.switching
		|| !sameMaintenancePath(review.launch.artifactPath, getInstallPath()) || !sameMaintenancePath(review.txDataPath, fxserverSettings.txDataPath)
		|| review.profile !== fxserverSettings.profile || review.launch.serverProfile !== review.profile
		|| JSON.stringify(review.launch.environment) !== JSON.stringify(launchRequest().environment)) {
		throw new Error("Server settings changed. Review a new maintenance plan.");
	}
}

export function invalidateMaintenanceReview() {
	preparationGeneration++;
	const token = maintenanceSession.review?.launchToken;
	maintenanceSession.review = null;
	reviewedAuthentication = null;
	if (token) void invoke("discard_fxserver_maintenance_launch", { token }).catch(() => {});
}

function launchRequest(): FxserverLaunchRequest {
	const environment: Record<string, string> = { ...readSavedEnvironment(), TXHOST_DATA_PATH: fxserverSettings.txDataPath };
	return { artifactPath: getInstallPath(), serverProfile: fxserverSettings.profile,
		environment: txHostFields.map(({ key }) => ({ key, value: (environment[key] ?? "").trim() })).filter((entry) => entry.value) };
}

function requireReviewedQueue(review: MaintenanceReview) {
	const plan = getResourcePlan(review.workspaceId);
	if (queueSignature(plan) !== review.queueSignature || plan.status === "running") throw new Error("The update queue changed. Review maintenance again.");
	for (const entry of plan.entries.filter((entry) => entry.status === "ready")) {
		const preference = getResourcePreference(entry.target);
		if (preference.ignored || preference.pinnedVersion !== null || Date.now() - entry.preview.createdAt * 1000 >= 30 * 60_000) throw new Error(`Review ${entry.name} again; it is pinned, ignored, or its preview expired.`);
		if (entry.target.workspaceId !== review.workspaceId || !sameMaintenancePath(entry.target.txDataPath, review.txDataPath) || entry.target.profile !== review.profile) throw new Error("A reviewed update belongs to a different server.");
	}
	return plan;
}

export async function prepareMaintenance(options: MaintenanceOptions) {
	if (preparing) throw new Error("A maintenance review is already being prepared. Wait for it to finish.");
	if (maintenanceSession.run?.status === "running") throw new Error("Maintenance is already running.");
	invalidateMaintenanceReview();
	const generation = preparationGeneration;
	preparing = true;
	let launchToken = "";
	try {
		const authentication = databaseSession.validated;
		if (!authentication || !databaseSession.credentials || !isDatabaseSessionValidated(databaseSession.credentials)) throw new Error("Validate the database connection before reviewing maintenance.");
		const workspaceId = workspaceSession.activeId;
		const revision = workspaceSession.revision;
		const launch = launchRequest();
		const scope: MaintenanceScope = { workspaceId, workspaceRevision: revision, launch, txDataPath: fxserverSettings.txDataPath, profile: fxserverSettings.profile };
		const assertCurrent = () => {
			currentReview(scope);
			if (generation !== preparationGeneration) throw new Error("Maintenance review was cancelled. Prepare a new plan.");
		};
		if (!launch.artifactPath || !fxserverSettings.txDataPath || !fxserverSettings.profile) throw new Error("Choose an artifact folder, txData folder, and server profile first.");
		if (!options.scheduleIds.length) throw new Error("Choose at least one database backup. Configure backup destinations in Backups & Restore first.");
		loadResourcePreferences(workspaceId);
		const status = await invoke<FxserverStatus>("get_fxserver_status");
		assertCurrent();
		if (!status.running || !status.pid || !sameMaintenancePath(status.artifactPath ?? "", launch.artifactPath)) throw new Error("Start this workspace's server through Manage Server before reviewing maintenance.");
		const overview = await invoke<BackupOverview>("get_backup_manager", { workspaceId });
		assertCurrent();
		if (overview.busy) throw new Error("Wait for the current backup or restore to finish.");
		const schedules = [...new Set(options.scheduleIds)].map((id) => {
			const schedule = overview.schedules.find((item) => item.config.id === id && item.config.workspaceId === workspaceId);
			if (!schedule) throw new Error("A selected backup schedule no longer exists.");
			return { ...schedule.config };
		});
		const bridge = await invoke<BridgeStatus>("get_live_bridge_status");
		assertCurrent();
		if (options.waitForPlayers && (!bridge.connected || bridge.workspaceId !== workspaceId)) throw new Error("Waiting for players requires a connected Live Bridge in this workspace.");
		const plan = getResourcePlan(workspaceId);
		launchToken = await invoke<string>("prepare_fxserver_maintenance_launch", { request: launch, expectedPid: status.pid });
		assertCurrent();
		const review: MaintenanceReview = { ...scope, id: crypto.randomUUID(), launchToken, createdAt: Date.now(), pid: status.pid, schedules, updates: plan.entries.filter((entry) => entry.status === "ready").map((entry) => ({ id: entry.id, name: entry.name, files: entry.preview.changes.length, protected: entry.protectedPaths.length })),
			queueSignature: queueSignature(plan), options: { ...options, scheduleIds: schedules.map((item) => item.id) }, bridgeRequired: bridge.enabled && bridge.workspaceId === workspaceId,
			databaseTarget: { host: authentication.credentials.host, port: authentication.credentials.port, username: authentication.credentials.username } };
		currentReview(review);
		requireReviewedQueue(review);
		if (databaseSession.validated !== authentication) throw new Error("The database connection changed. Review maintenance again.");
		reviewedAuthentication = authentication;
		maintenanceSession.review = review;
		launchToken = "";
		return review;
	} finally {
		preparing = false;
		if (launchToken) await invoke("discard_fxserver_maintenance_launch", { token: launchToken }).catch(() => {});
	}
}

export async function runMaintenance(review: MaintenanceReview) {
	currentReview(review);
	if (review !== maintenanceSession.review || Date.now() - review.createdAt > 5 * 60_000) throw new Error("Review this maintenance plan again before applying it.");
	const credentials = databaseSession.credentials;
	if (!credentials || !isDatabaseSessionValidated(credentials) || !reviewedAuthentication || databaseSession.validated !== reviewedAuthentication) throw new Error("The database connection changed. Validate it and review maintenance again.");
	const login = { ...credentials };
	const run = createMaintenanceRun(review.options.announce, review.options.waitForPlayers);
	await trackExclusiveTask("run_maintenance_plan", "Run reviewed maintenance", async () => {
		maintenanceSession.workspaceId = review.workspaceId;
		maintenanceSession.run = run;
		maintenanceSession.review = null;
		reviewedAuthentication = null;
		maintenanceSession.snapshots = [];
		const activeRun = maintenanceSession.run;
		let started: FxserverLaunchResult | null = null;
		let backups: Awaited<ReturnType<typeof reserveMaintenanceBackups>> | undefined;
		const check = () => { currentReview(review); backups?.assertActive(); };
		const checkDispatch = () => {
			check();
			if (activeRun.cancelRequested) throw new Error("Maintenance stopped at a safe boundary before the next action.");
		};
		const requireStopped = async () => { if ((await invoke<FxserverStatus>("get_fxserver_status")).running) throw new Error("The managed server is running. Maintenance stopped before making further changes."); };
		try {
			await executeMaintenance(activeRun, {
				validate: async () => {
					requireReviewedQueue(review);
					const status = await invoke<FxserverStatus>("get_fxserver_status");
					if (!status.running || status.pid !== review.pid) throw new Error("The server process changed. Review maintenance again.");
					await invoke("validate_fxserver_maintenance_launch", { token: review.launchToken });
					const overview = await invoke<BackupOverview>("get_backup_manager", { workspaceId: review.workspaceId });
					if (overview.busy || review.schedules.some((schedule) => JSON.stringify(overview.schedules.find((item) => item.config.id === schedule.id)?.config) !== JSON.stringify(schedule))) throw new Error("Backup settings changed or a backup is running. Review maintenance again.");
					await invoke("validate_mariadb_credentials", { credentials: login });
					checkDispatch();
					backups = await reserveMaintenanceBackups(review.workspaceId, review.schedules.map((schedule) => schedule.id));
					const reserved = await invoke<BackupOverview>("get_backup_manager", { workspaceId: review.workspaceId });
					checkDispatch();
					if (review.schedules.some((schedule) => JSON.stringify(reserved.schedules.find((item) => item.config.id === schedule.id)?.config) !== JSON.stringify(schedule))) throw new Error("Backup settings changed before the reservation. Review maintenance again.");
					return "Server, backup destinations, database login, and reviewed updates verified.";
				},
				announce: async () => {
					const password = await invoke<string | null>("get_fxserver_rcon_password", { workspaceId: review.workspaceId });
					if (!password) throw new Error("Save an RCON password in Manage Server before announcing maintenance.");
					const env = readSavedEnvironment();
					const rcon = { host: "127.0.0.1", port: Number(env.TXHOST_RCON_PORT || 30120), password };
					await invoke("verify_managed_rcon_endpoint", { expectedPid: review.pid, rcon });
					check();
					if (activeRun.cancelRequested) throw new Error("Announcement cancelled; the server was not stopped.");
					await invoke("send_fxserver_command", { request: { command: "say Server maintenance is starting. Please reconnect in a few minutes.", rcon } });
					return "Announcement sent. Delivery depends on the server's chat resource.";
				},
				players: async () => {
					const deadline = Date.now() + 10 * 60_000;
					while (Date.now() < deadline) {
						check();
						if (activeRun.cancelRequested) throw new Error("Waiting for players was cancelled; the server was not stopped.");
						const bridge = await invoke<BridgeStatus>("get_live_bridge_status");
						if (bridge.workspaceId !== review.workspaceId || !bridge.connected || !bridge.snapshot || !bridge.receivedAt || Date.now() - bridge.receivedAt > 15_000) throw new Error("Live Bridge data is unavailable or stale. The server was not stopped.");
						if (bridge.snapshot.playerCount === 0) return "Live Bridge reported no connected players.";
						await delay(2_000);
					}
					throw new Error("Players are still connected after ten minutes. Review maintenance again when appropriate.");
				},
				stop: async () => {
					const status = await invoke<FxserverStatus>("get_fxserver_status");
					if (!status.running || status.pid !== review.pid) throw new Error("The server process changed before shutdown.");
					checkDispatch();
					await invoke("stop_fxserver_for_maintenance", { token: review.launchToken }); await requireStopped();
					return "Managed server stopped. Automatic crash recovery is disarmed by the normal stop action.";
				},
				backup: async () => {
					for (const schedule of review.schedules) {
						check(); await requireStopped();
						checkDispatch();
						const snapshot = await invoke<BackupSnapshot>("run_scheduled_backup_now", { workspaceId: review.workspaceId, scheduleId: schedule.id, credentials: login, reservationToken: backups!.token });
						maintenanceSession.snapshots.push(snapshot);
					}
					return `${review.schedules.length} database backup(s) completed while the managed server was stopped.`;
				},
				updates: async () => {
					const plan = requireReviewedQueue(review);
					await runReviewedUpdates(plan, review.workspaceId, { preference: getResourcePreference,
						apply: async (target, previewId, protectedPaths) => {
							check(); await requireStopped();
							checkDispatch();
							return invoke<ResourceSnapshot>("apply_resource_update", { request: { target, previewId, protectedPaths } });
						}, discard: (previewId) => invoke("discard_resource_preview", { previewId }) });
					if (plan.status !== "completed" || plan.error) throw new Error("Resource updates did not complete. Inspect the update queue and snapshots before restarting.");
					return `${review.updates.length} individually reviewed update(s) applied. No SQL migrations were run.`;
				},
				preflight: async () => {
					await requireStopped();
					const report = await invoke<PreflightReport>("run_fxserver_preflight", { request: { artifactPath: review.launch.artifactPath, txDataPath: review.txDataPath, profile: review.profile, credentials: login, checkPorts: true } });
					if (report.blocking) throw new Error(`Preflight found ${report.errorCount} blocking issue(s). Open Diagnostics; the server was left stopped.`);
					return `No blocking issues; ${report.warningCount} advisory warning(s).`;
				},
				start: async () => {
					await requireStopped(); checkDispatch();
					started = await invoke<FxserverLaunchResult>("start_fxserver_after_maintenance", { token: review.launchToken });
					return "Server process started.";
				},
				health: async () => {
					const began = Date.now();
					while (Date.now() - began < 90_000) {
						check();
						if (activeRun.cancelRequested) throw new Error("Startup observation cancelled. The server remains running; check Manage Server.");
						const status = await invoke<FxserverStatus>("get_fxserver_status");
						if (!status.running || status.pid !== started?.pid) throw new Error("The server exited or restarted during verification. Check Server Logs and Health & Recovery.");
						if (Date.now() - began >= 15_000) {
							if (!review.bridgeRequired) return "Process stayed running for 15 seconds. Game readiness was not verified without Live Bridge.";
							const bridge = await invoke<BridgeStatus>("get_live_bridge_status");
							if (bridge.workspaceId === review.workspaceId && bridge.connected && bridge.receivedAt && bridge.receivedAt >= began && bridge.snapshot) return "Process stayed running and Live Bridge reconnected. Review logs for resource-specific errors.";
						}
						await delay(2_000);
					}
					throw new Error("The process is running, but Live Bridge did not reconnect within 90 seconds. Check Server Logs.");
				},
			}, check);
		} finally {
			login.password = "";
			const cleanupErrors: string[] = [];
			try { await backups?.release(); } catch { cleanupErrors.push("Backup reservation release was not confirmed; it expires automatically after two minutes without renewal."); }
			try { await invoke("discard_fxserver_maintenance_launch", { token: review.launchToken }); } catch { cleanupErrors.push("Launch review cleanup was not confirmed. Review the server state before another plan."); }
			if (cleanupErrors.length) {
				activeRun.status = "paused";
				activeRun.error = [activeRun.error, ...cleanupErrors].filter(Boolean).join(" ");
			}
			log(`Maintenance ${activeRun.status}.`, { scope: "fxserver.maintenance", level: activeRun.status === "completed" ? "success" : "warn" });
			if (cleanupErrors.length) throw new Error(activeRun.error);
		}
	});
}
