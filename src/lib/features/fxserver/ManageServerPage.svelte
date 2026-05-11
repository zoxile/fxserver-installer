<script lang="ts">
	import ActivityIcon from "@lucide/svelte/icons/activity";
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import CheckCircle2Icon from "@lucide/svelte/icons/check-circle-2";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import PlayIcon from "@lucide/svelte/icons/play";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import ServerIcon from "@lucide/svelte/icons/server";
	import SquareIcon from "@lucide/svelte/icons/square";
	import { onDestroy, onMount } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Progress } from "$lib/components/ui/progress/index.js";
	import { chooseInstallFolder } from "$lib/core/selectFolder";
	import { getInstallPath, loadInstallPath, setInstallPath } from "$lib/core/paths.svelte";
	import { getInstalledWindowsArtifactInfo, type InstalledArtifactInfo } from "$lib/modules/artifact";
	import { getFxserverStatus, startFxserver, stopFxserver, type FxserverStatus } from "$lib/modules/fxserver";
	import TxHostFieldInput from "./TxHostFieldInput.svelte";
	import { txHostFields, txHostGroups } from "./fxserverEnv";

	const envStorageKey = "fxserver.manage.env";
	const profileStorageKey = "fxserver.manage.serverProfile";

	let artifactPath = $state("");
	let artifact = $state<InstalledArtifactInfo | null>(null);
	let status = $state<FxserverStatus>({ running: false });
	let envValues = $state<Record<string, string>>(emptyEnvironment());
	let serverProfile = $state("");
	let busy = $state(false);
	let starting = $state(false);
	let stopping = $state(false);
	let error = $state("");
	let message = $state("");
	let refreshTimer: number | undefined;

	const activeEnvCount = $derived(Object.values(envValues).filter((value) => value.trim()).length + (serverProfile.trim() ? 1 : 0));
	const canStart = $derived(Boolean(artifactPath.trim()) && !status.running && !starting && !busy);

	onMount(() => {
		loadInstallPath();
		artifactPath = getInstallPath();
		loadSavedEnvironment();
		void refreshAll();
		refreshTimer = window.setInterval(() => {
			if (status.running) void refreshStatus(false);
		}, 2500);
	});

	onDestroy(() => {
		if (refreshTimer) window.clearInterval(refreshTimer);
	});

	function loadSavedEnvironment() {
		try {
			const saved = localStorage.getItem(envStorageKey);
			envValues = { ...emptyEnvironment(), ...(saved ? JSON.parse(saved) : {}) };
			serverProfile = localStorage.getItem(profileStorageKey) ?? "";
		} catch {
			envValues = emptyEnvironment();
			serverProfile = "";
		}
	}

	function emptyEnvironment() {
		return Object.fromEntries(txHostFields.map((field) => [field.key, ""]));
	}

	function saveEnvironment() {
		const trimmedEntries = Object.entries(envValues)
			.map(([key, value]) => [key, value.trim()])
			.filter(([, value]) => value);
		localStorage.setItem(envStorageKey, JSON.stringify(Object.fromEntries(trimmedEntries)));
		localStorage.setItem(profileStorageKey, serverProfile.trim());
	}

	function updateArtifactPath(event: Event) {
		artifactPath = (event.currentTarget as HTMLInputElement).value;
		setInstallPath(artifactPath);
	}

	async function chooseFolder() {
		error = "";
		message = "";

		const selectedPath = await chooseInstallFolder();
		artifactPath = selectedPath ?? getInstallPath();
		await refreshArtifact();
	}

	async function refreshAll() {
		busy = true;
		error = "";
		try {
			await Promise.all([refreshArtifact(), refreshStatus(false)]);
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		} finally {
			busy = false;
		}
	}

	async function refreshArtifact() {
		artifact = artifactPath.trim() ? await getInstalledWindowsArtifactInfo(artifactPath.trim()) : null;
	}

	async function refreshStatus(showMessage = true) {
		status = await getFxserverStatus();
		if (showMessage) message = status.running ? "FXServer status refreshed." : "FXServer is not running from this app.";
	}

	function launchEnvironment() {
		return txHostFields
			.map((field) => ({ key: field.key, value: (envValues[field.key] ?? "").trim() }))
			.filter((entry) => entry.value);
	}

	async function startServer() {
		error = "";
		message = "";
		starting = true;

		try {
			saveEnvironment();
			await startFxserver({
				artifactPath: artifactPath.trim(),
				environment: launchEnvironment(),
				serverProfile: serverProfile.trim() || null,
			});
			await refreshStatus(false);
			message = "FXServer started with the selected TXHOST environment.";
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		} finally {
			starting = false;
		}
	}

	async function stopServer() {
		error = "";
		message = "";
		stopping = true;

		try {
			await stopFxserver();
			await refreshStatus(false);
			message = "FXServer stopped.";
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		} finally {
			stopping = false;
		}
	}

	function bytes(value?: number | null) {
		if (!value) return "0 MB";
		const units = ["B", "KB", "MB", "GB", "TB"];
		let scaled = value;
		let unit = 0;
		while (scaled >= 1024 && unit < units.length - 1) {
			scaled /= 1024;
			unit += 1;
		}
		return `${scaled.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
	}

	function uptime(seconds?: number | null) {
		if (!seconds) return "0s";
		const hours = Math.floor(seconds / 3600);
		const minutes = Math.floor((seconds % 3600) / 60);
		const secs = seconds % 60;
		return [hours ? `${hours}h` : "", minutes ? `${minutes}m` : "", `${secs}s`].filter(Boolean).join(" ");
	}

	function startedAt(value?: string | null) {
		if (!value) return "Not started";
		const seconds = Number(value);
		if (!Number.isFinite(seconds)) return value;
		return new Date(seconds * 1000).toLocaleString();
	}
</script>

<section class="space-y-6">
	<div class="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">FXServer</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Manage Server</h1>
			<p class="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">Start FXServer from your selected artifact folder with txAdmin TXHOST environment variables, then watch the process while it runs.</p>
		</div>
		<Button variant="outline" onclick={() => refreshAll()} disabled={busy || starting || stopping} title="Refresh artifact details and FXServer process status">
			<RefreshCwIcon class={busy ? "animate-spin" : undefined} />
			Refresh
		</Button>
	</div>

	{#if error}
		<div class="rounded-sm border border-red-400/30 bg-red-400/10 px-4 py-3 text-sm text-red-100">
			<div class="flex items-start gap-2">
				<AlertCircleIcon class="mt-0.5 size-4 shrink-0" />
				<p>{error}</p>
			</div>
		</div>
	{:else if message}
		<div class="rounded-sm border border-emerald-400/30 bg-emerald-400/10 px-4 py-3 text-sm text-emerald-100">
			<div class="flex items-start gap-2">
				<CheckCircle2Icon class="mt-0.5 size-4 shrink-0" />
				<p>{message}</p>
			</div>
		</div>
	{/if}

	<div class="grid gap-4 xl:grid-cols-12">
		<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5 xl:col-span-7">
			<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
			<Card.Header class="border-b border-border pb-4">
				<div class="flex items-center gap-3">
					<div class="flex size-9 shrink-0 items-center justify-center rounded-sm border border-sky-400/30 bg-sky-400/10 text-sky-200">
						<ServerIcon class="size-4" />
					</div>
					<div>
						<Card.Title>Artifact Path</Card.Title>
						<Card.Description>Use the saved artifact folder or choose the folder that contains FXServer.exe.</Card.Description>
					</div>
				</div>
			</Card.Header>
			<Card.Content class="space-y-4">
				<div class="grid gap-3 md:grid-cols-[1fr_auto]">
					<label class="grid gap-2">
						<span class="text-xs font-medium text-muted-foreground">Artifact Folder</span>
						<Input value={artifactPath} oninput={updateArtifactPath} placeholder="C:\FXServer\server" title="Folder that contains FXServer.exe" class="rounded-sm font-mono" />
					</label>
					<div class="flex items-end">
						<Button variant="outline" onclick={chooseFolder} disabled={starting || stopping} title="Pick the FXServer artifact folder">
							<FolderOpenIcon />
							Browse
						</Button>
					</div>
				</div>

				<div class="grid gap-3 md:grid-cols-3">
					<div class="rounded-sm border border-border bg-background/70 p-3">
						<p class="text-xs text-muted-foreground">Installed Build</p>
						<p class="mt-1 font-mono text-xl font-semibold text-foreground">{artifact?.version ?? (artifact?.installed ? "Unknown" : "None")}</p>
					</div>
					<div class="rounded-sm border border-border bg-background/70 p-3">
						<p class="text-xs text-muted-foreground">Detection</p>
						<p class="mt-1 text-xl font-semibold text-foreground">{artifact?.detectionSource ?? "None"}</p>
					</div>
					<div class="rounded-sm border border-border bg-background/70 p-3">
						<p class="text-xs text-muted-foreground">Executable</p>
						<p class="mt-1 text-xl font-semibold text-foreground">{artifact?.hasFxserverExecutable ? "Found" : "Missing"}</p>
					</div>
				</div>

				{#if artifact?.citizenServerImplPath}
					<p class="truncate rounded-sm border border-border bg-background/60 px-3 py-2 font-mono text-xs text-muted-foreground" title={artifact.citizenServerImplPath}>
						{artifact.citizenServerImplPath}
					</p>
				{/if}
			</Card.Content>
		</Card.Root>

		<Card.Root class="group relative flex overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5 xl:col-span-5">
			<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
			<div class="flex w-full flex-col">
				<Card.Header class="border-b border-border pb-4">
					<div class="flex items-center justify-between gap-3">
						<div>
							<Card.Title>Process Status</Card.Title>
							<Card.Description>{status.running ? `Running as PID ${status.pid}` : "Not started from this app."}</Card.Description>
						</div>
						<div class={`rounded-sm border px-2 py-1 text-xs font-semibold ${status.running ? "border-emerald-400/30 bg-emerald-400/10 text-emerald-200" : "border-border bg-background text-muted-foreground"}`}>
							{status.running ? "RUNNING" : "STOPPED"}
						</div>
					</div>
				</Card.Header>
				<Card.Content class="flex flex-1 flex-col gap-4">
					<div class="grid gap-3 sm:grid-cols-2">
						<div class="rounded-sm border border-border bg-background/70 p-3">
							<p class="text-xs text-muted-foreground">Started</p>
							<p class="mt-1 text-sm font-medium text-foreground">{startedAt(status.startedAt)}</p>
						</div>
						<div class="rounded-sm border border-border bg-background/70 p-3">
							<p class="text-xs text-muted-foreground">Uptime</p>
							<p class="mt-1 font-mono text-sm font-medium text-foreground">{uptime(status.uptimeSeconds)}</p>
						</div>
					</div>

					<div class="space-y-4 rounded-sm border border-border bg-background/70 p-3">
						<div>
							<div class="mb-2 flex justify-between text-xs">
								<span class="text-muted-foreground">CPU</span>
								<span class="font-mono text-foreground">{(status.resources?.cpuPercent ?? 0).toFixed(2)}%</span>
							</div>
							<Progress value={status.resources?.cpuPercent ?? 0} class="h-2 rounded-xs" indicatorClass="bg-sky-300" />
						</div>
						<div>
							<div class="mb-2 flex justify-between text-xs">
								<span class="text-muted-foreground">Memory</span>
								<span class="font-mono text-foreground">{bytes(status.resources?.memoryBytes)} / {bytes(status.resources?.totalMemoryBytes)}</span>
							</div>
							<Progress value={status.resources?.memoryPercent ?? 0} class="h-2 rounded-xs" indicatorClass="bg-emerald-300" />
						</div>
						<div class="grid grid-cols-2 gap-3 text-xs">
							<div class="rounded-sm border border-border bg-card/60 p-2">
								<p class="text-muted-foreground">Threads</p>
								<p class="mt-1 font-mono text-foreground">{status.resources?.threadCount ?? 0}</p>
							</div>
							<div class="rounded-sm border border-border bg-card/60 p-2">
								<p class="text-muted-foreground">Handles</p>
								<p class="mt-1 font-mono text-foreground">{status.resources?.handleCount ?? 0}</p>
							</div>
						</div>
					</div>

					<div class="mt-auto grid gap-2 sm:grid-cols-3">
						<Button onclick={startServer} disabled={!canStart} title="Start FXServer.exe with the configured TXHOST variables">
							{#if starting}
								<LoaderCircleIcon class="animate-spin" />
							{:else}
								<PlayIcon />
							{/if}
							Start
						</Button>
						<Button variant="destructive" onclick={stopServer} disabled={!status.running || stopping} title="Stop the FXServer process started by this app">
							{#if stopping}
								<LoaderCircleIcon class="animate-spin" />
							{:else}
								<SquareIcon />
							{/if}
							Stop
						</Button>
						<Button variant="outline" onclick={() => refreshStatus()} disabled={busy} title="Refresh FXServer process usage">
							<ActivityIcon />
							Status
						</Button>
					</div>
				</Card.Content>
			</div>
		</Card.Root>
	</div>

	<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
		<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
		<Card.Header class="border-b border-border pb-4">
			<div class="flex flex-col justify-between gap-3 md:flex-row md:items-start">
				<div>
					<Card.Title>TXHOST Environment</Card.Title>
					<Card.Description>These variables are applied only to the FXServer boot process started from this page.</Card.Description>
				</div>
				<div class="rounded-sm border border-primary/30 bg-primary/10 px-2 py-1 text-xs font-semibold text-primary">{activeEnvCount} active</div>
			</div>
		</Card.Header>
		<Card.Content class="space-y-5">
			<label class="grid gap-2 rounded-sm border border-amber-400/20 bg-amber-400/5 p-3">
				<span class="flex items-center justify-between gap-3">
					<span class="text-xs font-semibold text-amber-100">Legacy Server Profile</span>
					<span class="font-mono text-[10px] text-amber-200/70">+set serverProfile</span>
				</span>
				<span class="text-xs leading-5 text-muted-foreground">Optional compatibility argument for older txAdmin profile flows. Separate txData folders are preferred for new setups.</span>
				<Input bind:value={serverProfile} placeholder="default" title="Optional legacy txAdmin serverProfile argument" class="rounded-sm font-mono text-xs" oninput={saveEnvironment} />
			</label>

			{#each txHostGroups as group}
				<div class="space-y-3">
					<div class="flex items-center gap-2">
						<div class="h-px flex-1 bg-border"></div>
						<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">{group}</p>
						<div class="h-px flex-1 bg-border"></div>
					</div>
					<div class="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
						{#each txHostFields.filter((field) => field.group === group) as field}
							<TxHostFieldInput field={field} bind:value={envValues[field.key]} />
						{/each}
					</div>
				</div>
			{/each}

			<div class="flex flex-wrap justify-end gap-2">
				<Button variant="outline" onclick={saveEnvironment} title="Save TXHOST environment values locally for the next launch">Save Environment</Button>
				<Button
					variant="outline"
					onclick={() => {
						envValues = emptyEnvironment();
						serverProfile = "";
						saveEnvironment();
					}}
					title="Clear all TXHOST environment values"
				>
					Clear
				</Button>
			</div>
		</Card.Content>
	</Card.Root>
</section>
