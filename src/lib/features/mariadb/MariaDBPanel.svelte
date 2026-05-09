<script lang="ts">
	import { onMount } from "svelte";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import ConnectionCard from "./ConnectionCard.svelte";
	import ExistingUsersCard from "./ExistingUsersCard.svelte";
	import InstallConfigCard from "./InstallConfigCard.svelte";
	import MariaDBNotice from "./MariaDBNotice.svelte";
	import QueryConsole from "./QueryConsole.svelte";
	import StatusOverview from "./StatusOverview.svelte";
	import UserManagementCard from "./UserManagementCard.svelte";
	import {
		deleteMariaDBUser,
		executeMariaDBQuery,
		getMariaDBStatus,
		installMariaDB,
		listMariaDBUsers,
		restartMariaDBService,
		saveMariaDBUser,
		startMariaDBService,
		stopMariaDBService,
		updateMariaDBUser,
		type MariaDBCredentials,
		type MariaDBInstallOptions,
		type MariaDBQueryResult,
		type MariaDBStatus,
		type MariaDBUser,
	} from "$lib/modules/mariadb";

	let status = $state<MariaDBStatus | null>(null);
	let busy = $state(false);
	let message = $state("");
	let error = $state("");
	let query = $state("SELECT VERSION();");
	let queryResult = $state<MariaDBQueryResult | null>(null);
	let users = $state<MariaDBUser[]>([]);
	let editingUser = $state<{
		username: string;
		host: string;
		password: string;
		database: string;
		privileges: string;
	} | null>(null);
	let credentials = $state<MariaDBCredentials>({
		host: "127.0.0.1",
		port: 3306,
		username: "root",
		password: "",
		database: "",
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
		const timer = window.setTimeout(() => {
			void refreshStatus();
		}, 80);

		return () => window.clearTimeout(timer);
	});

	async function runTask<T>(task: () => Promise<T>, success: string, after?: (value: T) => void) {
		busy = true;
		error = "";
		message = "";

		try {
			const value = await task();
			after?.(value);
			message = success;
			return value;
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		} finally {
			busy = false;
		}
	}

	async function refreshStatus() {
		await runTask(getMariaDBStatus, "MariaDB status refreshed.", (value) => (status = value));
	}

	async function install() {
		await runTask(() => installMariaDB(installOptions), "MariaDB installer completed.");
		await refreshStatus();
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
		await runTask(() => listMariaDBUsers(credentials), "MariaDB users refreshed.", (value) => (users = value));
	}

	function editUser(user: MariaDBUser) {
		editingUser = {
			username: user.username,
			host: user.host,
			password: "",
			database: credentials.database || "",
			privileges: "ALL PRIVILEGES",
		};
	}

	async function saveExistingUser() {
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
	}

	async function removeExistingUser(user: MariaDBUser) {
		await runTask(() => deleteMariaDBUser(credentials, user.username, user.host), "Database user deleted.");
		await refreshUsers();
	}

	async function executeQuery() {
		await runTask(
			() => executeMariaDBQuery(credentials, query),
			"Query executed.",
			(value) => (queryResult = value),
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

	<div class="grid items-start gap-4 xl:grid-cols-12">
		{#if status && !status.installed}
			<div class="xl:col-span-12">
				<InstallConfigCard bind:installOptions {busy} onInstall={install} />
			</div>
		{/if}
		<div class="xl:col-span-6">
			<StatusOverview {status} {busy} onRefresh={refreshStatus} onStart={startService} onStop={stopService} onRestart={restartService} />
		</div>
		<div class="xl:col-span-6">
			<ConnectionCard bind:credentials />
		</div>
		<div class="xl:col-span-6">
			<UserManagementCard bind:userConfig {busy} onSave={saveUser} />
		</div>
		<div class="xl:col-span-6">
			<ExistingUsersCard
				{busy}
				{users}
				bind:editingUser
				onRefresh={refreshUsers}
				onEdit={editUser}
				onSave={saveExistingUser}
				onDelete={removeExistingUser}
			/>
		</div>
		<div class="xl:col-span-12">
			<QueryConsole bind:query {busy} result={queryResult} onExecute={executeQuery} />
		</div>
	</div>
</section>
