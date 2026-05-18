<script lang="ts">
	import { onMount, tick } from "svelte";
	import BackupCard from "./BackupCard.svelte";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import ConnectionCard from "./ConnectionCard.svelte";
	import ExistingUsersCard from "./ExistingUsersCard.svelte";
	import InstallConfigCard from "./InstallConfigCard.svelte";
	import MariaDBNotice from "./MariaDBNotice.svelte";
	import QueryConsole from "./QueryConsole.svelte";
	import StatusOverview from "./StatusOverview.svelte";
	import UserManagementCard from "./UserManagementCard.svelte";
	import { databaseSession, rememberDatabaseCredentials } from "$lib/core/databaseSession.svelte";
	import { log } from "$lib/core/logger.svelte";
	import {
		deleteMariaDBUser,
		executeMariaDBQuery,
		backupMariaDB,
		getDefaultMariaDBBackupOutputDir,
		getMariaDBStatus,
		getMariaDBUserAccess,
		installMariaDB,
		listMariaDBDatabases,
		listMariaDBTables,
		listMariaDBUsers,
		restartMariaDBService,
		saveMariaDBUser,
		startMariaDBService,
		stopMariaDBService,
		updateMariaDBUser,
		validateMariaDBCredentials,
		type MariaDBBackupOptions,
		type MariaDBCredentials,
		type MariaDBInstallOptions,
		type MariaDBQueryResult,
		type MariaDBStatus,
		type MariaDBUser,
		type MariaDBUserAccess,
	} from "$lib/modules/mariadb";

	let status = $state<MariaDBStatus | null>(null);
	let busy = $state(false);
	let message = $state("");
	let error = $state("");
	let credentialsReady = $state(false);
	let connectionError = $state("");
	let query = $state("SELECT VERSION();");
	let queryResult = $state<MariaDBQueryResult | null>(null);
	let queryDatabase = $state("__global__");
	let databases = $state<string[]>([]);
	let backupTables = $state<string[]>([]);
	let backupMode = $state<"database" | "tables" | "all">("database");
	let backupDatabaseName = $state("");
	let selectedBackupTable = $state("");
	let loadedBackupTablesFor = $state("");
	let loadingBackupTablesFor = $state("");
	let backupTableRequestId = 0;
	let backupOptions = $state<MariaDBBackupOptions>({
		outputDir: "",
		fileName: "",
		database: "",
		tables: [],
		allDatabases: false,
		schemaOnly: false,
		dataOnly: false,
		includeRoutines: true,
		includeTriggers: true,
		includeEvents: false,
		singleTransaction: true,
		addDropStatements: false,
		whereClause: "",
	});
	let users = $state<MariaDBUser[]>([]);
	let selectedUser = $state<MariaDBUser | null>(null);
	let selectedAccess = $state<MariaDBUserAccess | null>(null);
	let editingUser = $state<{
		username: string;
		host: string;
		password: string;
		database: string;
		privileges: string;
	} | null>(null);
	let credentials = $state<MariaDBCredentials>({
		host: databaseSession.credentials?.host ?? "127.0.0.1",
		port: databaseSession.credentials?.port ?? 3306,
		username: databaseSession.credentials?.username ?? "root",
		password: databaseSession.credentials?.password ?? "",
		database: databaseSession.credentials?.database ?? "",
	});
	let installOptions = $state<MariaDBInstallOptions>({
		rootPassword: "",
		serviceName: "MariaDB",
		port: 3306,
		installDir: "",
		dataDir: "",
		allowRemoteRootAccess: false,
		createAnonymousUser: false,
		skipNetworking: false,
		optimizeForTransactions: true,
		useUtf8: true,
		pageSize: "",
		bufferPoolSize: "",
		installHeidiSql: false,
		installDevelopmentFiles: false,
	});
	let userConfig = $state({
		username: "fxserver",
		password: "",
		host: "localhost",
		database: "fxserver",
		privileges: "ALL PRIVILEGES",
	});

	onMount(() => {
		void initializeBackupOutputDir();

		const timer = window.setTimeout(() => {
			void refreshStatus(false);
		}, 80);

		return () => window.clearTimeout(timer);
	});

	$effect(() => {
		const database = backupDatabaseName.trim();
		const canLoadTables = credentialsReady && backupMode === "tables" && database;

		if (!canLoadTables) {
			backupTables = [];
			selectedBackupTable = "";
			loadedBackupTablesFor = "";
			loadingBackupTablesFor = "";
			return;
		}

		if (loadedBackupTablesFor === database || loadingBackupTablesFor === database) return;

		void refreshBackupTables(database);
	});

	async function initializeBackupOutputDir() {
		if (backupOptions.outputDir.trim()) return;

		const outputDir = await getDefaultMariaDBBackupOutputDir();
		if (outputDir && !backupOptions.outputDir.trim()) {
			backupOptions.outputDir = outputDir;
		}
	}

	async function runTask<T>(task: () => Promise<T>, success: string, after?: (value: T) => void) {
		busy = true;
		error = "";
		message = "";
		await tick();

		try {
			const value = await task();
			after?.(value);
			message = success;
			return value;
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
			log("MariaDB panel task failed.", { level: "error", scope: "mariadb.ui", detail: error });
		} finally {
			busy = false;
		}
	}

	async function refreshStatus(force = true) {
		await runTask(
			() => getMariaDBStatus(force),
			force ? "MariaDB status refreshed." : "MariaDB status loaded.",
			(value) => (status = value),
		);
	}

	async function install() {
		const result = await runTask(() => installMariaDB(installOptions), "MariaDB installer completed.");
		if (result !== undefined) {
			await refreshStatus(true);
			message = status?.installed ? "MariaDB installer completed and the installation was detected." : result;
		}
	}

	async function startService() {
		await runTask(
			() => startMariaDBService(status?.serviceName),
			"MariaDB service started.",
			(value) => (status = value),
		);
	}

	async function stopService() {
		await runTask(
			() => stopMariaDBService(status?.serviceName),
			"MariaDB service stopped.",
			(value) => (status = value),
		);
	}

	async function restartService() {
		await runTask(
			() => restartMariaDBService(status?.serviceName),
			"MariaDB service restarted.",
			(value) => (status = value),
		);
	}

	async function saveUser() {
		if (!credentialsReady) {
			error = "Apply valid admin credentials before adding MariaDB users.";
			log("MariaDB add-user action blocked until credentials are applied.", { level: "warn", scope: "mariadb.ui" });
			return;
		}

		await runTask(
			() =>
				saveMariaDBUser(credentials, {
					...userConfig,
					privileges: userConfig.privileges
						.split(",")
						.map((privilege) => privilege.trim())
						.filter(Boolean),
				}),
			"Database user added.",
		);
	}

	async function refreshUsers() {
		if (!credentialsReady) {
			error = "Apply valid admin credentials before refreshing MariaDB users.";
			log("MariaDB user refresh blocked until credentials are applied.", { level: "warn", scope: "mariadb.ui" });
			return;
		}

		await runTask(
			() => listMariaDBUsers(credentials),
			"MariaDB users refreshed.",
			(value) => {
				users = value;
				if (selectedUser && !value.some((user) => user.username === selectedUser?.username && user.host === selectedUser?.host)) {
					selectedUser = null;
					selectedAccess = null;
					editingUser = null;
				}
			},
		);
	}

	async function editUser(user: MariaDBUser) {
		if (!credentialsReady) {
			error = "Apply valid admin credentials before editing MariaDB users.";
			log(`MariaDB edit action blocked for ${user.username}@${user.host}.`, { level: "warn", scope: "mariadb.ui" });
			return;
		}

		log(`MariaDB user selected for editing: ${user.username}@${user.host}.`, { scope: "mariadb.ui" });
		selectedUser = user;
		selectedAccess = null;
		editingUser = {
			username: user.username,
			host: user.host,
			password: "",
			database: credentials.database || "",
			privileges: "ALL PRIVILEGES",
		};
		await refreshUserAccess(user);
	}

	async function refreshUserAccess(user = selectedUser) {
		if (!credentialsReady) return;
		if (!user) return;
		await runTask(
			() => getMariaDBUserAccess(credentials, user.username, user.host),
			"MariaDB user access refreshed.",
			(value) => (selectedAccess = value),
		);
	}

	async function applyCredentials() {
		credentialsReady = false;
		connectionError = "";
		databases = [];
		backupTables = [];
		loadedBackupTablesFor = "";
		loadingBackupTablesFor = "";
		selectedAccess = null;
		log("MariaDB admin credentials changed; refreshing status and users.", { scope: "mariadb.ui", detail: `${credentials.username}@${credentials.host}:${credentials.port}` });
		await refreshStatus(true);

		busy = true;
		error = "";
		message = "";

		try {
			await validateMariaDBCredentials(credentials);
			const [loadedUsers, loadedDatabases] = await Promise.all([listMariaDBUsers(credentials), listMariaDBDatabases(credentials)]);
			users = loadedUsers;
			databases = loadedDatabases;
			credentialsReady = true;
			rememberDatabaseCredentials(credentials);
			message = "Admin credentials applied.";
			if (credentials.database && loadedDatabases.includes(credentials.database)) {
				backupDatabaseName ||= credentials.database;
				queryDatabase = credentials.database;
			} else if (!backupDatabaseName && loadedDatabases.length) {
				backupDatabaseName = loadedDatabases[0];
			}
			await refreshUserAccess();
		} catch (caught) {
			connectionError = caught instanceof Error ? caught.message : String(caught);
			error = connectionError;
			log("MariaDB credentials rejected.", { level: "error", scope: "mariadb.ui", detail: connectionError });
		} finally {
			busy = false;
		}
	}

	async function refreshBackupTables(database: string) {
		const requestId = ++backupTableRequestId;
		loadingBackupTablesFor = database;

		try {
			const tables = await listMariaDBTables(credentials, database);
			if (requestId !== backupTableRequestId || backupDatabaseName.trim() !== database) return;

			backupTables = tables;
			backupOptions.tables = backupOptions.tables.filter((table) => tables.includes(table));
			if (selectedBackupTable && !tables.includes(selectedBackupTable)) {
				selectedBackupTable = "";
			}
			loadedBackupTablesFor = database;
		} catch (caught) {
			const detail = caught instanceof Error ? caught.message : String(caught);
			log("MariaDB table list refresh failed.", { level: "error", scope: "mariadb.ui", detail });
			backupTables = [];
			loadedBackupTablesFor = "";
		} finally {
			if (requestId === backupTableRequestId && loadingBackupTablesFor === database) {
				loadingBackupTablesFor = "";
			}
		}
	}

	async function saveExistingUser() {
		if (!credentialsReady) {
			error = "Apply valid admin credentials before updating MariaDB users.";
			log("MariaDB user update blocked until credentials are applied.", { level: "warn", scope: "mariadb.ui" });
			return;
		}

		if (!editingUser) return;
		const config = editingUser;

		await runTask(
			() =>
				updateMariaDBUser(credentials, {
					...config,
					password: config.password || null,
					privileges: config.privileges
						.split(",")
						.map((privilege) => privilege.trim())
						.filter(Boolean),
				}),
			"Database user updated.",
		);
		await refreshUsers();
		await refreshUserAccess();
	}

	async function removeExistingUser(user: MariaDBUser) {
		if (!credentialsReady) {
			error = "Apply valid admin credentials before deleting MariaDB users.";
			log(`MariaDB delete action blocked for ${user.username}@${user.host}.`, { level: "warn", scope: "mariadb.ui" });
			return;
		}

		await runTask(() => deleteMariaDBUser(credentials, user.username, user.host), "Database user deleted.");
		await refreshUsers();
	}

	async function executeQuery() {
		if (!credentialsReady) {
			error = "Apply valid admin credentials before running MariaDB queries.";
			log("MariaDB query execution blocked until credentials are applied.", { level: "warn", scope: "mariadb.ui" });
			return;
		}

		await runTask(
			() =>
				executeMariaDBQuery(
					{
						...credentials,
						database: queryDatabase === "__global__" ? null : queryDatabase,
					},
					query,
				),
			"Query executed.",
			(value) => (queryResult = value),
		);
	}

	async function backupDatabase() {
		if (!credentialsReady) {
			error = "Apply valid admin credentials before creating MariaDB backups.";
			log("MariaDB backup action blocked until credentials are applied.", { level: "warn", scope: "mariadb.ui" });
			return;
		}

		if (backupMode === "tables" && backupOptions.tables.length === 0) {
			error = "Choose at least one table to back up.";
			return;
		}

		await runTask(
			() =>
				backupMariaDB(credentials, {
					...backupOptions,
					allDatabases: backupMode === "all",
					database: backupMode === "all" ? null : backupDatabaseName.trim() || credentials.database || null,
					tables: backupMode === "tables" ? backupOptions.tables : [],
					fileName: backupOptions.fileName?.trim() || null,
					whereClause: backupOptions.whereClause?.trim() || null,
				}),
			"MariaDB backup created.",
			(value) => {
				message = `Backup created: ${value.path}`;
			},
		);
	}
</script>

<section class="space-y-6">
	<div class="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">MariaDB</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Database Management</h1>
			<p class="mt-2 max-w-2xl text-sm text-muted-foreground">Install MariaDB, inspect the local Windows service, manage database users, and run SQL.</p>
		</div>
		<div class="inline-flex items-center gap-2 rounded-sm border border-border bg-card px-3 py-2 text-xs text-muted-foreground">
			{#if busy}
				<LoaderCircleIcon class="size-3.5 animate-spin" />
			{/if}
			{busy ? "Working..." : "Ready"}
		</div>
	</div>

	{#if message || error}
		<MariaDBNotice {message} {error} onDismiss={() => ((message = ""), (error = ""))} />
	{/if}

	<div class="grid gap-4 xl:grid-cols-12">
		{#if status && !status.installed}
			<div class="xl:col-span-12">
				<InstallConfigCard bind:installOptions {busy} onInstall={install} />
			</div>
		{/if}
		<div class="xl:col-span-6">
			<StatusOverview {status} {busy} onRefresh={refreshStatus} onStart={startService} onStop={stopService} onRestart={restartService} />
		</div>
		<div class="xl:col-span-6">
			<ConnectionCard bind:credentials {busy} {credentialsReady} {connectionError} onApply={applyCredentials} />
		</div>
		<div class="xl:col-span-5">
			<UserManagementCard bind:userConfig {busy} {credentialsReady} {databases} onSave={saveUser} />
		</div>
		<div class="xl:col-span-7 xl:row-span-2">
			<ExistingUsersCard
				{busy}
				{credentialsReady}
				{users}
				{selectedUser}
				{selectedAccess}
				bind:editingUser
				{databases}
				onRefresh={refreshUsers}
				onEdit={editUser}
				onSave={saveExistingUser}
				onDelete={removeExistingUser}
			/>
		</div>
		<div class="xl:col-span-12">
			<BackupCard
				bind:backupOptions
				bind:backupMode
				bind:backupDatabase={backupDatabaseName}
				bind:selectedTable={selectedBackupTable}
				{busy}
				canBackup={credentialsReady}
				{databases}
				tables={backupTables}
				onBackup={backupDatabase}
			/>
		</div>
		<div class="xl:col-span-12">
			<QueryConsole bind:query bind:selectedDatabase={queryDatabase} {busy} canExecute={credentialsReady} {databases} result={queryResult} onExecute={executeQuery} />
		</div>
	</div>
</section>
