<script lang="ts">
	import ActivityIcon from "@lucide/svelte/icons/activity";
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import BarChart3Icon from "@lucide/svelte/icons/bar-chart-3";
	import CheckCircle2Icon from "@lucide/svelte/icons/check-circle-2";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import PlayIcon from "@lucide/svelte/icons/play";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import SendIcon from "@lucide/svelte/icons/send";
	import ServerIcon from "@lucide/svelte/icons/server";
	import SquareIcon from "@lucide/svelte/icons/square";
	import { scaleUtc } from "d3-scale";
	import { curveNatural } from "d3-shape";
	import { AreaChart } from "layerchart";
	import { onDestroy, onMount, tick } from "svelte";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import PasswordInput from "$lib/components/ui/password-input.svelte";
	import { Progress } from "$lib/components/ui/progress/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import * as Chart from "$lib/components/ui/chart/index.js";
	import * as ToggleGroup from "$lib/components/ui/toggle-group/index.js";
	import { chooseFolder as chooseAnyFolder, chooseInstallFolder } from "$lib/core/selectFolder";
	import { getInstallPath, loadInstallPath, setInstallPath } from "$lib/core/paths.svelte";
	import { getInstalledWindowsArtifactInfo, type InstalledArtifactInfo } from "$lib/modules/artifact";
	import {
		getFxserverStatus,
		getFxserverTerminal,
		getSavedFxserverRconPassword,
		clearFxserverRconPassword,
		saveFxserverRconPassword,
		sendFxserverCommand,
		startFxserver,
		stopFxserver,
		type FxserverRconConfig,
		type FxserverStatus,
		type FxserverTerminalEntry,
	} from "$lib/modules/fxserver";
	import TxHostFieldInput from "./TxHostFieldInput.svelte";
	import { sensitiveTxHostKeys, txHostFields, txHostGroups } from "./fxserverEnv";
	import { fxserverSettings, loadFxserverSettings, readSavedEnvironment, refreshTxDataProfiles, setServerProfile, setTxDataPath, writeSavedEnvironment } from "./fxserverSettings.svelte";

	type MetricWindow = "30s" | "5m" | "10m" | "30m" | "1h";
	type ResourceSample = {
		timestamp: number;
		cpuPercent: number;
		memoryPercent: number;
		memoryBytes: number;
	};
	type ResourceExtremes = {
		cpuHigh: number | null;
		cpuLow: number | null;
		memoryHigh: number | null;
		memoryLow: number | null;
	};

	const metricWindows: { value: MetricWindow; label: string; milliseconds: number }[] = [
		{ value: "30s", label: "30s", milliseconds: 30_000 },
		{ value: "5m", label: "5m", milliseconds: 5 * 60_000 },
		{ value: "10m", label: "10m", milliseconds: 10 * 60_000 },
		{ value: "30m", label: "30m", milliseconds: 30 * 60_000 },
		{ value: "1h", label: "1h", milliseconds: 60 * 60_000 },
	];
	const resourceHistoryLimitMs = 60 * 60_000;
	const performanceChartConfig = {
		cpu: { label: "CPU", color: "var(--chart-1)" },
		memory: { label: "RAM", color: "var(--chart-2)" },
	} satisfies Chart.ChartConfig;
	const chartAxisProps = {
		xAxis: {
			format: (value: Date) => value.toLocaleTimeString([], { minute: "2-digit", second: "2-digit" }),
		},
		yAxis: { format: (value: number) => `${value}%` },
		grid: { y: true },
		area: {
			curve: curveNatural,
			fillOpacity: 0.28,
			line: { class: "stroke-2" },
		},
	};

	let artifactPath = $state("");
	let artifact = $state<InstalledArtifactInfo | null>(null);
	let status = $state<FxserverStatus>({ running: false });
	let terminalEntries = $state<FxserverTerminalEntry[]>([]);
	let terminalCommand = $state("");
	let rconConfig = $state<FxserverRconConfig>({
		host: "127.0.0.1",
		port: 30120,
		password: "",
	});
	let envValues = $state<Record<string, string>>(emptyEnvironment());
	let serverProfile = $state("");
	let storageReady = false;
	let busy = $state(false);
	let starting = $state(false);
	let stopping = $state(false);
	let error = $state("");
	let message = $state("");
	let refreshTimer: number | undefined;
	let terminalTimer: number | undefined;
	let uptimeTimer: number | undefined;
	let rconPasswordSaveTimer: number | undefined;
	let nowSeconds = $state(Math.floor(Date.now() / 1000));
	let terminalViewport: HTMLDivElement | null = null;
	let terminalRefreshPending = false;
	let rconPasswordLoaded = false;
	let lastSavedRconPassword = "";
	let metricWindow = $state<MetricWindow>("5m");
	let resourceSamples = $state<ResourceSample[]>([]);
	let resourceExtremes = $state<ResourceExtremes>({
		cpuHigh: null,
		cpuLow: null,
		memoryHigh: null,
		memoryLow: null,
	});

	const activeEnvCount = $derived(Object.values(envValues).filter((value) => value.trim()).length + (serverProfile.trim() ? 1 : 0));
	const canStart = $derived(Boolean(artifactPath.trim()) && !status.running && !starting && !busy);
	const txHostEditableFields = $derived(txHostFields.filter((field) => field.key !== "TXHOST_DATA_PATH"));
	const displayedUptimeSeconds = $derived(status.running ? Math.max(0, nowSeconds - (startedAtSeconds(status.startedAt) ?? nowSeconds)) : (status.uptimeSeconds ?? 0));
	const visibleResourceSamples = $derived(filterResourceSamples(resourceSamples, metricWindow, nowSeconds * 1000));
	const resourceChartData = $derived(
		visibleResourceSamples.map((sample) => ({
			date: new Date(sample.timestamp),
			cpu: sample.cpuPercent,
			memory: sample.memoryPercent,
		})),
	);
	const currentResourceSample = $derived(resourceSamples.at(-1) ?? null);
	const profileOptions = $derived([
		...(fxserverSettings.hasRootLogs ? [{ value: "", label: "Root logs folder" }] : []),
		...fxserverSettings.profiles.map((profile) => ({ value: profile, label: profile })),
	]);

	let autoScrollTerminal = $state(true);
	let lastTerminalEntryId = $state<number | null>(null);
	const terminalBufferLimit = 1000;

	onMount(() => {
		void initializePage();
		refreshTimer = window.setInterval(() => {
			if (status.running) void refreshStatus(false);
		}, 2500);
		uptimeTimer = window.setInterval(() => {
			nowSeconds = Math.floor(Date.now() / 1000);
		}, 1000);
		terminalTimer = window.setInterval(() => {
			void refreshTerminal({ scrollToBottom: false });
		}, 1000);
	});

	onDestroy(() => {
		if (refreshTimer) window.clearInterval(refreshTimer);
		if (terminalTimer) window.clearInterval(terminalTimer);
		if (uptimeTimer) window.clearInterval(uptimeTimer);
		if (rconPasswordSaveTimer) window.clearTimeout(rconPasswordSaveTimer);
	});

	$effect(() => {
		JSON.stringify(envValues);
		serverProfile;
		JSON.stringify(rconConfig);

		if (storageReady) {
			saveEnvironment();
			if (rconPasswordLoaded) scheduleRconPasswordSave();
		}
	});

	$effect(() => {
		const lastEntry = terminalEntries.at(-1);

		if (!lastEntry || lastEntry.id === lastTerminalEntryId) return;

		lastTerminalEntryId = lastEntry.id;

		if (autoScrollTerminal) void scrollTerminalToBottom();
	});

	$effect(() => {
		const sharedTxDataPath = fxserverSettings.txDataPath;
		const sharedProfile = fxserverSettings.profile;

		if (!storageReady) return;

		if ((envValues.TXHOST_DATA_PATH ?? "") !== sharedTxDataPath) {
			envValues = { ...envValues, TXHOST_DATA_PATH: sharedTxDataPath };
		}

		if (serverProfile !== sharedProfile) {
			serverProfile = sharedProfile;
		}
	});

	async function initializePage() {
		loadInstallPath();
		loadFxserverSettings();
		artifactPath = getInstallPath();
		loadSavedEnvironment();
		await loadSecureRconPassword();
		storageReady = true;
		void refreshAll();
		void refreshTxDataProfiles();
		void refreshTerminal({ reset: true, scrollToBottom: false });
	}

	function loadSavedEnvironment() {
		try {
			const saved = readSavedEnvironment();
			envValues = { ...emptyEnvironment(), ...saved, TXHOST_DATA_PATH: fxserverSettings.txDataPath };
			rconConfig = {
				host: saved.TXHOST_RCON_HOST || "127.0.0.1",
				port: Number.parseInt(saved.TXHOST_RCON_PORT || "30120", 10) || 30120,
				password: "",
			};
			serverProfile = fxserverSettings.profile;
		} catch {
			envValues = emptyEnvironment();
			rconConfig = { host: "127.0.0.1", port: 30120, password: "" };
			serverProfile = "";
		}
	}

	async function loadSecureRconPassword() {
		try {
			const password = await getSavedFxserverRconPassword();
			rconConfig = { ...rconConfig, password };
			lastSavedRconPassword = password;
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		} finally {
			rconPasswordLoaded = true;
		}
	}

	function emptyEnvironment() {
		return Object.fromEntries(txHostFields.map((field) => [field.key, ""]));
	}

	function saveEnvironment() {
		const trimmedEntries = Object.entries(envValues)
			.map(([key, value]) => [key, value.trim()])
			.filter(([key, value]) => value && !sensitiveTxHostKeys.has(key));
		writeSavedEnvironment({
			...Object.fromEntries(trimmedEntries),
			TXHOST_RCON_HOST: rconConfig.host.trim(),
			TXHOST_RCON_PORT: String(rconConfig.port || 30120),
		});
		setTxDataPath((envValues.TXHOST_DATA_PATH ?? "").trim());
		setServerProfile(serverProfile.trim());
	}

	function scheduleRconPasswordSave() {
		if (rconConfig.password === lastSavedRconPassword) return;
		if (rconPasswordSaveTimer) window.clearTimeout(rconPasswordSaveTimer);
		rconPasswordSaveTimer = window.setTimeout(async () => {
			try {
				if (rconConfig.password) {
					await saveFxserverRconPassword(rconConfig.password);
				} else {
					await clearFxserverRconPassword();
				}
				lastSavedRconPassword = rconConfig.password;
			} catch (caught) {
				error = caught instanceof Error ? caught.message : String(caught);
			}
		}, 400);
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

	async function chooseTxDataFolder() {
		error = "";
		message = "";

		const selectedPath = await chooseAnyFolder();
		if (!selectedPath) return;

		envValues = { ...envValues, TXHOST_DATA_PATH: selectedPath };
		setTxDataPath(selectedPath);
		await refreshTxDataProfiles();
	}

	async function handleTxDataInput(event: Event) {
		const nextPath = (event.currentTarget as HTMLInputElement).value;
		envValues = { ...envValues, TXHOST_DATA_PATH: nextPath };
		setTxDataPath(nextPath);
		await refreshTxDataProfiles();
	}

	function handleProfileChange(profile: string) {
		serverProfile = profile;
		setServerProfile(profile);
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
		nowSeconds = Math.floor(Date.now() / 1000);
		recordResourceSample(status);
		if (showMessage) message = status.running ? "FXServer status refreshed." : "FXServer is not running from this app.";
	}

	function recordResourceSample(nextStatus: FxserverStatus) {
		const resources = nextStatus.resources;
		if (!nextStatus.running || !resources) return;

		const timestamp = Date.now();
		const sample: ResourceSample = {
			timestamp,
			cpuPercent: clampPercent(resources.cpuPercent),
			memoryPercent: clampPercent(resources.memoryPercent),
			memoryBytes: Math.max(0, resources.memoryBytes ?? 0),
		};
		const cutoff = timestamp - resourceHistoryLimitMs;
		const previous = resourceSamples.at(-1);
		const retained = resourceSamples.filter((entry) => entry.timestamp >= cutoff);

		if (previous && timestamp - previous.timestamp < 750) {
			resourceSamples = [...retained.slice(0, -1), sample];
		} else {
			resourceSamples = [...retained, sample];
		}

		resourceExtremes = {
			cpuHigh: maxNullable(resourceExtremes.cpuHigh, sample.cpuPercent),
			cpuLow: minNullable(resourceExtremes.cpuLow, sample.cpuPercent),
			memoryHigh: maxNullable(resourceExtremes.memoryHigh, sample.memoryPercent),
			memoryLow: minNullable(resourceExtremes.memoryLow, sample.memoryPercent),
		};
	}

	async function refreshTerminal(options: { reset?: boolean; scrollToBottom?: boolean } = {}) {
		if (terminalRefreshPending) return;
		terminalRefreshPending = true;
		try {
			const reset = options.reset ?? false;
			const afterId = reset ? null : (terminalEntries.at(-1)?.id ?? null);
			const result = await getFxserverTerminal(reset ? terminalBufferLimit : 250, afterId);
			mergeTerminalEntries(result.entries, reset);
			if (options.scrollToBottom ?? true) {
				autoScrollTerminal = true;
				await scrollTerminalToBottom();
			}
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		} finally {
			terminalRefreshPending = false;
		}
	}

	function mergeTerminalEntries(entries: FxserverTerminalEntry[], reset: boolean) {
		if (reset) {
			terminalEntries = entries.slice(-terminalBufferLimit);
			lastTerminalEntryId = terminalEntries.at(-1)?.id ?? null;
			return;
		}

		if (!entries.length) return;

		const existingIds = new Set(terminalEntries.map((entry) => entry.id));
		const merged = [...terminalEntries, ...entries.filter((entry) => !existingIds.has(entry.id))];
		terminalEntries = merged.slice(-terminalBufferLimit);
	}

	async function scrollTerminalToBottom() {
		await tick();
		if (!terminalViewport) return;
		terminalViewport.scrollTop = Math.max(0, terminalViewport.scrollHeight - terminalViewport.clientHeight);
	}

	function launchEnvironment() {
		return txHostFields.map((field) => ({ key: field.key, value: (envValues[field.key] ?? "").trim() })).filter((entry) => entry.value);
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
			await Promise.all([refreshStatus(false), refreshTerminal({ reset: true })]);
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
			await Promise.all([refreshStatus(false), refreshTerminal({ reset: true })]);
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

	function percent(value?: number | null) {
		return value === null || value === undefined ? "--" : `${value.toFixed(2)}%`;
	}

	function clampPercent(value?: number | null) {
		if (!Number.isFinite(value ?? Number.NaN)) return 0;
		return Math.max(0, Math.min(100, Number(value)));
	}

	function maxNullable(current: number | null, value: number) {
		return current === null ? value : Math.max(current, value);
	}

	function minNullable(current: number | null, value: number) {
		return current === null ? value : Math.min(current, value);
	}

	function filterResourceSamples(samples: ResourceSample[], windowValue: MetricWindow, now: number) {
		const selectedWindow = metricWindows.find((entry) => entry.value === windowValue) ?? metricWindows[1];
		const cutoff = now - selectedWindow.milliseconds;
		return samples.filter((sample) => sample.timestamp >= cutoff);
	}

	function uptime(seconds?: number | null) {
		if (!seconds) return "0s";
		const hours = Math.floor(seconds / 3600);
		const minutes = Math.floor((seconds % 3600) / 60);
		const secs = seconds % 60;
		return [hours ? `${hours}h` : "", minutes ? `${minutes}m` : "", `${secs}s`].filter(Boolean).join(" ");
	}

	function startedAtSeconds(value?: string | null) {
		if (!value) return null;
		const seconds = Number(value);
		return Number.isFinite(seconds) ? seconds : null;
	}

	function startedAt(value?: string | null) {
		if (!value) return "Not started";
		const seconds = startedAtSeconds(value);
		if (seconds === null) return value;
		return new Date(seconds * 1000).toLocaleString();
	}

	function terminalTime(value: string) {
		const seconds = Number(value);
		if (!Number.isFinite(seconds)) return value;
		return new Date(seconds * 1000).toLocaleTimeString(undefined, {
			hour: "2-digit",
			minute: "2-digit",
			second: "2-digit",
		});
	}

	function terminalStreamClass(stream: string) {
		return (
			{
				stdout: "border-sky-400/30 bg-sky-400/10 text-sky-200",
			stderr: "border-red-400/30 bg-red-400/10 text-red-200",
			system: "border-emerald-400/30 bg-emerald-400/10 text-emerald-200",
			command: "border-amber-400/30 bg-amber-400/10 text-amber-200",
			rcon: "border-violet-400/30 bg-violet-400/10 text-violet-200",
		}[stream] ?? "border-border bg-background text-muted-foreground"
	);
}

	function updateRconPort(event: Event) {
		const value = Number.parseInt((event.currentTarget as HTMLInputElement).value, 10);
		rconConfig = { ...rconConfig, port: Number.isFinite(value) ? value : 30120 };
	}

	function updateAutoScrollFromScroll() {
		if (!terminalViewport) return;
		const distanceFromBottom = terminalViewport.scrollHeight - terminalViewport.scrollTop - terminalViewport.clientHeight;
		autoScrollTerminal = distanceFromBottom < 28;
	}

	async function submitTerminalCommand() {
		const command = terminalCommand.trim();
		if (!command) return;

		try {
			await sendFxserverCommand(command, rconConfig);
			terminalCommand = "";
			await refreshTerminal({ scrollToBottom: true });
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		}
	}
</script>

<section class="space-y-6">
	<div class="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">FXServer</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Manage Server</h1>
			<p class="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">
				Start FXServer from your selected artifact folder with txAdmin TXHOST environment variables, then watch the process while it runs.
			</p>
		</div>
		<Button variant="outline" onclick={() => refreshAll()} disabled={busy || starting || stopping} title="Refresh artifact details and FXServer process status">
			<RefreshCwIcon class={busy ? "animate-spin" : undefined} />
			Refresh
		</Button>
	</div>

	{#if error}
		<Notice tone="error" message={error} onDismiss={() => (error = "")} class="px-4 py-3 text-sm" />
	{:else if message}
		<Notice tone="success" message={message} onDismiss={() => (message = "")} class="px-4 py-3 text-sm" />
	{/if}

	<div class="grid gap-4 xl:grid-cols-12">
		<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5 xl:col-span-7">
			<div
				class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
			></div>
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
						<p class="text-xs text-muted-foreground">Launch Ready</p>
						<p class="mt-1 text-xl font-semibold text-foreground">{artifact?.installed && artifact?.hasFxserverExecutable ? "Ready" : "Needs setup"}</p>
					</div>
					<div class="rounded-sm border border-border bg-background/70 p-3">
						<p class="text-xs text-muted-foreground">Executable</p>
						<p class="mt-1 text-xl font-semibold text-foreground">{artifact?.hasFxserverExecutable ? "Found" : "Missing"}</p>
					</div>
				</div>
			</Card.Content>
		</Card.Root>

		<Card.Root class="group relative flex overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5 xl:col-span-5">
			<div
				class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
			></div>
			<div class="flex w-full flex-col">
				<Card.Header class="border-b border-border pb-4">
					<div class="flex items-center justify-between gap-3">
						<div>
							<Card.Title>Process Status</Card.Title>
							<Card.Description>{status.running ? `Running as PID ${status.pid}` : "Not started from this app."}</Card.Description>
						</div>
						<div
							class={`rounded-sm border px-2 py-1 text-xs font-semibold ${status.running ? "border-emerald-400/30 bg-emerald-400/10 text-emerald-200" : "border-red-400/30 bg-red-400/10 text-red-200"}`}
						>
							{status.running ? "RUNNING" : "STOPPED"}
						</div>
					</div>
				</Card.Header>
				<Card.Content class="flex flex-1 flex-col gap-4 pt-5">
					<div class="grid gap-3 sm:grid-cols-2">
						<div class="rounded-sm border border-border bg-background/70 p-3">
							<p class="text-xs text-muted-foreground">Started</p>
							<p class="mt-1 text-sm font-medium text-foreground">{startedAt(status.startedAt)}</p>
						</div>
						<div class="rounded-sm border border-border bg-background/70 p-3">
							<p class="text-xs text-muted-foreground">Uptime</p>
							<p class="mt-1 font-mono text-sm font-medium text-foreground">{uptime(displayedUptimeSeconds)}</p>
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
		<div
			class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
		></div>
		<Card.Header class="border-b border-border pb-4">
			<div class="flex flex-col justify-between gap-3 xl:flex-row xl:items-start">
				<div class="flex items-start gap-3">
					<div class="flex size-9 shrink-0 items-center justify-center rounded-sm border border-violet-400/30 bg-violet-400/10 text-violet-200">
						<BarChart3Icon class="size-4" />
					</div>
					<div>
						<Card.Title>Performance History</Card.Title>
						<Card.Description>CPU and RAM samples from the FXServer process started by this app.</Card.Description>
					</div>
				</div>
				<ToggleGroup.Root
					type="single"
					value={metricWindow}
					onValueChange={(value) => {
						if (typeof value === "string" && value) metricWindow = value as MetricWindow;
					}}
					class="flex-wrap justify-start xl:justify-end"
				>
					{#each metricWindows as window}
						<ToggleGroup.Item value={window.value} title={`Show the last ${window.label}`}>
							{window.label}
						</ToggleGroup.Item>
					{/each}
				</ToggleGroup.Root>
			</div>
		</Card.Header>
		<Card.Content class="grid gap-4 pt-5 xl:grid-cols-2">
			<div class="rounded-sm border border-border bg-background/70 p-4">
				<div class="mb-4 flex items-start justify-between gap-3">
					<div>
						<p class="text-sm font-semibold text-foreground">CPU Usage</p>
						<p class="mt-1 text-xs text-muted-foreground">Current process CPU over the selected window.</p>
					</div>
					<div class="text-right">
						<p class="font-mono text-2xl font-semibold text-sky-100">{percent(currentResourceSample?.cpuPercent)}</p>
						<p class="text-xs text-muted-foreground">current</p>
					</div>
				</div>

				<div class="relative h-52 overflow-hidden rounded-sm border border-sky-400/15 bg-black/30 p-2">
					<Chart.Container config={performanceChartConfig} class="h-full w-full">
						<AreaChart
							data={resourceChartData}
							x="date"
							xScale={scaleUtc()}
							yDomain={[0, 100]}
							yPadding={[0, 8]}
							series={[
								{
									key: "cpu",
									label: "CPU",
									color: "var(--color-cpu)",
								},
							]}
							props={chartAxisProps}
						>
							<Chart.Tooltip
								slot="tooltip"
								indicator="dot"
								labelFormatter={(value) => (value instanceof Date ? value.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" }) : "")}
								valueFormatter={(value) => (typeof value === "number" ? `${value.toFixed(2)}%` : String(value ?? ""))}
							/>
						</AreaChart>
					</Chart.Container>
					{#if visibleResourceSamples.length < 2}
						<div class="absolute inset-0 flex items-center justify-center p-4 text-center text-sm text-muted-foreground">Start FXServer to collect CPU samples.</div>
					{/if}
				</div>

				<div class="mt-3 grid grid-cols-3 gap-2 text-xs">
					<div class="rounded-sm border border-border bg-card/60 p-2">
						<p class="text-muted-foreground">All-time high</p>
						<p class="mt-1 font-mono text-foreground">{percent(resourceExtremes.cpuHigh)}</p>
					</div>
					<div class="rounded-sm border border-border bg-card/60 p-2">
						<p class="text-muted-foreground">All-time low</p>
						<p class="mt-1 font-mono text-foreground">{percent(resourceExtremes.cpuLow)}</p>
					</div>
					<div class="rounded-sm border border-border bg-card/60 p-2">
						<p class="text-muted-foreground">Samples</p>
						<p class="mt-1 font-mono text-foreground">{visibleResourceSamples.length}</p>
					</div>
				</div>
			</div>

			<div class="rounded-sm border border-border bg-background/70 p-4">
				<div class="mb-4 flex items-start justify-between gap-3">
					<div>
						<p class="text-sm font-semibold text-foreground">RAM Usage</p>
						<p class="mt-1 text-xs text-muted-foreground">Memory percentage and current process allocation.</p>
					</div>
					<div class="text-right">
						<p class="font-mono text-2xl font-semibold text-emerald-100">{percent(currentResourceSample?.memoryPercent)}</p>
						<p class="text-xs text-muted-foreground">{bytes(currentResourceSample?.memoryBytes)}</p>
					</div>
				</div>

				<div class="relative h-52 overflow-hidden rounded-sm border border-emerald-400/15 bg-black/30 p-2">
					<Chart.Container config={performanceChartConfig} class="h-full w-full">
						<AreaChart
							data={resourceChartData}
							x="date"
							xScale={scaleUtc()}
							yDomain={[0, 100]}
							yPadding={[0, 8]}
							series={[
								{
									key: "memory",
									label: "RAM",
									color: "var(--color-memory)",
								},
							]}
							props={chartAxisProps}
						>
							<Chart.Tooltip
								slot="tooltip"
								indicator="dot"
								labelFormatter={(value) => (value instanceof Date ? value.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" }) : "")}
								valueFormatter={(value) => (typeof value === "number" ? `${value.toFixed(2)}%` : String(value ?? ""))}
							/>
						</AreaChart>
					</Chart.Container>
					{#if visibleResourceSamples.length < 2}
						<div class="absolute inset-0 flex items-center justify-center p-4 text-center text-sm text-muted-foreground">Start FXServer to collect RAM samples.</div>
					{/if}
				</div>

				<div class="mt-3 grid grid-cols-3 gap-2 text-xs">
					<div class="rounded-sm border border-border bg-card/60 p-2">
						<p class="text-muted-foreground">All-time high</p>
						<p class="mt-1 font-mono text-foreground">{percent(resourceExtremes.memoryHigh)}</p>
					</div>
					<div class="rounded-sm border border-border bg-card/60 p-2">
						<p class="text-muted-foreground">All-time low</p>
						<p class="mt-1 font-mono text-foreground">{percent(resourceExtremes.memoryLow)}</p>
					</div>
					<div class="rounded-sm border border-border bg-card/60 p-2">
						<p class="text-muted-foreground">Samples</p>
						<p class="mt-1 font-mono text-foreground">{visibleResourceSamples.length}</p>
					</div>
				</div>
			</div>
		</Card.Content>
	</Card.Root>

	<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
		<div
			class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
		></div>
		<Card.Header class="border-b border-border pb-4">
			<div class="flex flex-col justify-between gap-3 md:flex-row md:items-start">
				<div>
					<Card.Title>Server Console</Card.Title>
					<Card.Description>Live output from the hidden FXServer process, with command input sent through RCON.</Card.Description>
				</div>
				<div class="rounded-sm border border-border bg-background px-2 py-1 text-xs font-semibold text-muted-foreground">{terminalEntries.length} lines</div>
			</div>
		</Card.Header>
		<Card.Content class="space-y-3">
			<div class="flex items-center justify-between gap-3 rounded-sm border border-border bg-muted/30 px-3 py-2">
				<div>
					<div class="text-sm font-medium">Console output</div>
					<p class="text-xs text-muted-foreground">Showing the latest {terminalEntries.length} buffered lines.</p>
				</div>

				<div class="flex items-center gap-2">
					<Button
						variant="outline"
						size="sm"
						onclick={() => {
							autoScrollTerminal = true;
							void scrollTerminalToBottom();
						}}
						title="Jump to the latest console line"
					>
						Bottom
					</Button>
					<label
						class="flex cursor-pointer select-none items-center gap-2 rounded-sm border border-border bg-background px-2.5 py-1.5 text-xs font-medium text-muted-foreground shadow-xs transition-colors hover:bg-transparent hover:text-foreground"
					>
						<Checkbox
							bind:checked={autoScrollTerminal}
							class="size-4 rounded-lg border-border data-[state=checked]:border-primary data-[state=checked]:bg-primary data-[state=checked]:text-primary-foreground"
						/>
						<span>Auto-scroll</span>
					</label>
				</div>
			</div>

			<div bind:this={terminalViewport} onscroll={updateAutoScrollFromScroll} class="h-104 overflow-auto rounded-sm border border-border bg-black/40 font-mono text-xs">
				{#if terminalEntries.length}
					<div class="min-w-0 py-1">
						{#each terminalEntries as item (item.id)}
							<div class="flex min-h-6 min-w-0 items-center gap-2 px-3 text-muted-foreground">
								<span class="w-20 shrink-0 truncate text-[11px] text-muted-foreground/80">
									{terminalTime(item.timestamp)}
								</span>

								<span class={`w-16 shrink-0 truncate rounded-xs border px-1.5 py-0.5 text-center text-[10px] leading-none font-semibold uppercase ${terminalStreamClass(item.stream)}`}>
									{item.stream}
								</span>

								<span class="min-w-0 flex-1 truncate text-foreground" title={item.line}>
									{item.line}
								</span>
							</div>
						{/each}
					</div>
				{:else}
					<div class="flex h-full items-center justify-center p-3 text-center text-sm text-muted-foreground">Start FXServer to see console output here.</div>
				{/if}
			</div>

			<div class="grid gap-3 rounded-sm border border-border bg-muted/20 p-3 md:grid-cols-[minmax(0,1fr)_8rem_minmax(0,1fr)]">
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">RCON Host</span>
					<Input bind:value={rconConfig.host} placeholder="127.0.0.1" title="Host where FXServer accepts RCON connections." class="rounded-sm font-mono text-xs" />
				</label>
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Port</span>
					<Input value={String(rconConfig.port)} oninput={updateRconPort} type="number" min="1" max="65535" placeholder="30120" title="FXServer RCON port." class="rounded-sm font-mono text-xs" />
				</label>
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Password</span>
					<PasswordInput bind:value={rconConfig.password} placeholder="server.cfg rcon_password" title="Value configured with rcon_password in server.cfg." class="rounded-sm font-mono text-xs" />
				</label>
			</div>

			<form
				class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]"
				onsubmit={(event) => {
					event.preventDefault();
					void submitTerminalCommand();
				}}
			>
				<Input
					bind:value={terminalCommand}
					placeholder="status, refresh, say hello..."
					title="Command to send to FXServer through RCON"
					disabled={!status.running || !rconConfig.password.trim()}
					class="rounded-sm font-mono text-xs"
				/>
				<Button type="submit" disabled={!status.running || !terminalCommand.trim() || !rconConfig.password.trim()} title="Send command through RCON">
					<SendIcon />
					Send
				</Button>
			</form>
		</Card.Content>
	</Card.Root>

	<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
		<div
			class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
		></div>
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
			<div class="grid gap-3 rounded-sm border border-sky-400/20 bg-sky-400/5 p-3 lg:grid-cols-[minmax(0,1fr)_minmax(14rem,0.45fr)]">
				<label class="grid gap-2">
					<span class="flex items-center justify-between gap-3">
						<span class="text-xs font-semibold text-sky-100">txData Path</span>
						<span class="font-mono text-[10px] text-sky-200/70">TXHOST_DATA_PATH</span>
					</span>
					<span class="text-xs leading-5 text-muted-foreground">Shared txData folder used for profile detection and server log browsing.</span>
					<div class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
						<Input
							value={envValues.TXHOST_DATA_PATH ?? ""}
							oninput={(event) => {
								const nextPath = (event.currentTarget as HTMLInputElement).value;
								envValues = { ...envValues, TXHOST_DATA_PATH: nextPath };
								setTxDataPath(nextPath);
							}}
							onchange={handleTxDataInput}
							placeholder="C:\FiveM\txData"
							title="Folder containing txAdmin profile folders and logs."
							class="rounded-sm font-mono text-xs"
						/>
						<Button variant="outline" onclick={chooseTxDataFolder} title="Browse for the txData folder">
							<FolderOpenIcon />
							Browse
						</Button>
					</div>
				</label>

				<label class="grid gap-2">
					<span class="flex items-center justify-between gap-3">
						<span class="text-xs font-semibold text-sky-100">Profile</span>
						<span class="font-mono text-[10px] text-sky-200/70">{fxserverSettings.loadingProfiles ? "scanning" : `${fxserverSettings.profiles.length} found`}</span>
					</span>
					<span class="text-xs leading-5 text-muted-foreground">Shared profile used by Manage Server and Server Logs.</span>
					<Select.Root bind:value={serverProfile} type="single" items={profileOptions}>
						<Select.Trigger title="Choose the txData profile folder" class="w-full rounded-sm font-mono text-xs">
							{serverProfile || (fxserverSettings.hasRootLogs ? "Root logs folder" : "Choose profile")}
						</Select.Trigger>
						<Select.Content class="rounded-sm">
							{#if profileOptions.length}
								{#each profileOptions as option}
									<Select.Item value={option.value} label={option.label}>
										{option.label}
									</Select.Item>
								{/each}
							{:else}
								<Select.Item value="" label="No profiles detected" disabled>No profiles detected</Select.Item>
							{/if}
						</Select.Content>
					</Select.Root>
					{#if fxserverSettings.profileError}
						<span class="text-xs text-red-200">{fxserverSettings.profileError}</span>
					{/if}
				</label>
			</div>

			<label class="grid gap-2 rounded-sm border border-amber-400/20 bg-amber-400/5 p-3">
				<span class="flex items-center justify-between gap-3">
					<span class="text-xs font-semibold text-amber-100">Legacy Server Profile</span>
					<span class="font-mono text-[10px] text-amber-200/70">+set serverProfile</span>
				</span>
				<span class="text-xs leading-5 text-muted-foreground">Optional compatibility argument for older txAdmin profile flows. Separate txData folders are preferred for new setups.</span>
				<Input
					value={serverProfile}
					oninput={(event) => handleProfileChange((event.currentTarget as HTMLInputElement).value)}
					placeholder="default"
					title="Optional legacy txAdmin serverProfile argument"
					class="rounded-sm font-mono text-xs"
				/>
			</label>

			{#each txHostGroups as group}
				<div class="space-y-3">
					<div class="flex items-center gap-2">
						<div class="h-px flex-1 bg-border"></div>
						<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">{group}</p>
						<div class="h-px flex-1 bg-border"></div>
					</div>
					<div class="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
						{#each txHostEditableFields.filter((field) => field.group === group) as field}
							<TxHostFieldInput {field} sensitive={sensitiveTxHostKeys.has(field.key)} bind:value={envValues[field.key]} />
						{/each}
					</div>
				</div>
			{/each}

			<p class="rounded-sm border border-border bg-background/60 px-3 py-2 text-xs leading-5 text-muted-foreground">
				Sensitive values are applied when starting FXServer but are not saved locally: API token, CFX key, database user, database password, and default account.
			</p>

			<div class="grid gap-2 sm:grid-cols-2">
				<Button class="w-full rounded-sm" variant="outline" onclick={saveEnvironment} title="Save non-sensitive TXHOST values locally for the next launch">Save Environment</Button>
				<Button
					class="w-full rounded-sm"
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
