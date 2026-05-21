<script lang="ts">
	import ActivityIcon from "@lucide/svelte/icons/activity";
	import DownloadIcon from "@lucide/svelte/icons/download";
	import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
	import GitBranchIcon from "@lucide/svelte/icons/git-branch";
	import PlayIcon from "@lucide/svelte/icons/play";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import RotateCcwIcon from "@lucide/svelte/icons/rotate-cw";
	import SearchIcon from "@lucide/svelte/icons/search";
	import ShieldIcon from "@lucide/svelte/icons/shield";
	import SquareIcon from "@lucide/svelte/icons/square";
	import { onMount } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import PasswordInput from "$lib/components/ui/password-input.svelte";
	import { openExternalUrl } from "$lib/core/openExternal";
	import {
		clearFxserverRconPassword,
		getSavedFxserverRconPassword,
		queryFxserverResourceStates,
		saveFxserverRconPassword,
		scanFxserverResources,
		sendFxserverRconCommand,
		updateGithubResource,
		type FxserverRconConfig,
		type FxserverResourceInfo,
		type ResourceScanResult,
	} from "$lib/modules/fxserver";
	import { fxserverSettings, loadFxserverSettings, readSavedEnvironment } from "./fxserverSettings.svelte";

	type UpdateStatus = "unchecked" | "checking" | "up-to-date" | "update-available" | "no-repository" | "error";
	type RuntimeState = "running" | "stopped" | "missing" | "unknown" | string;

	type ResourceView = FxserverResourceInfo & {
		updateStatus: UpdateStatus;
		updateError: string;
		latestVersion: string;
		defaultBranch: string;
		latestManifestUrl: string;
		repositoryWebUrl: string;
		runtimeState: RuntimeState;
		updating: boolean;
	};

	let rcon = $state<FxserverRconConfig>({ host: "127.0.0.1", port: 30120, password: "" });
	let scanResult = $state<ResourceScanResult | null>(null);
	let resources = $state<ResourceView[]>([]);
	let search = $state("");
	let filter = $state<"all" | "updates" | "repos" | "missing-repo" | "running" | "unknown">("all");
	let scanning = $state(false);
	let checkingAll = $state(false);
	let refreshingStates = $state(false);
	let busyCommand = $state("");
	let message = $state("");
	let error = $state("");
	let recentCommands = $state<string[]>([]);
	let contextMenuResource = $state<ResourceView | null>(null);
	let contextMenuPosition = $state({ x: 0, y: 0 });

	const filteredResources = $derived(
		resources.filter((resource) => {
			const needle = search.trim().toLowerCase();
			const matchesSearch =
				!needle ||
				[resource.name, resource.repository ?? "", resource.version ?? "", resource.latestVersion, resource.path]
					.join(" ")
					.toLowerCase()
					.includes(needle);

			if (!matchesSearch) return false;
			if (filter === "updates") return resource.updateStatus === "update-available";
			if (filter === "repos") return Boolean(resource.repository);
			if (filter === "missing-repo") return !resource.repository;
			if (filter === "running") return resource.runtimeState === "running";
			if (filter === "unknown") return resource.runtimeState === "unknown";
			return true;
		}),
	);
	const resourcesWithRepositories = $derived(resources.filter((resource) => Boolean(resource.repository)).length);
	const updateCount = $derived(resources.filter((resource) => resource.updateStatus === "update-available").length);
	const runningCount = $derived(resources.filter((resource) => resource.runtimeState === "running").length);

	onMount(() => {
		void initialize();
	});

	async function initialize() {
		loadFxserverSettings();
		const saved = readSavedEnvironment();
		rcon = {
			host: saved.TXHOST_RCON_HOST || "127.0.0.1",
			port: Number.parseInt(saved.TXHOST_RCON_PORT || "30120", 10) || 30120,
			password: await getSavedFxserverRconPassword(),
		};

		if (fxserverSettings.txDataPath && fxserverSettings.profile) {
			await scanResources();
		}
	}

	async function scanResources(showMessage = true) {
		if (!fxserverSettings.txDataPath.trim() || !fxserverSettings.profile.trim()) {
			error = "Choose a txData path and profile in Manage Server or Configure Server before scanning resources.";
			return;
		}

		scanning = true;
		error = "";
		message = "";
		try {
			const previous = new Map(resources.map((resource) => [resource.path, resource]));
			const result = await scanFxserverResources({
				txDataPath: fxserverSettings.txDataPath,
				profile: fxserverSettings.profile,
			});
			scanResult = result;
			resources = result.resources.map((resource) => {
				const existing = previous.get(resource.path);
				return {
					...resource,
					updateStatus: resource.repository ? (existing?.updateStatus ?? "unchecked") : "no-repository",
					updateError: existing?.updateError ?? "",
					latestVersion: existing?.latestVersion ?? "",
					defaultBranch: existing?.defaultBranch ?? "",
					latestManifestUrl: existing?.latestManifestUrl ?? "",
					repositoryWebUrl: existing?.repositoryWebUrl ?? normalizeRepositoryWebUrl(resource.repository ?? ""),
					runtimeState: existing?.runtimeState ?? "unknown",
					updating: existing?.updating ?? false,
				};
			});
			if (showMessage) message = `Scanned ${resources.length} resource${resources.length === 1 ? "" : "s"}.`;
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		} finally {
			scanning = false;
		}
	}

	async function checkAllUpdates() {
		if (!resources.length) return;
		checkingAll = true;
		error = "";
		message = "";
		try {
			const candidates = resources.filter((resource) => resource.repository);
			await runLimited(candidates, 4, checkResourceUpdate);
			message = "Resource update checks completed.";
		} finally {
			checkingAll = false;
		}
	}

	async function checkResourceUpdate(resource: ResourceView) {
		if (!resource.repository) {
			patchResource(resource.path, { updateStatus: "no-repository", updateError: "No repository entry in fxmanifest." });
			return;
		}

		patchResource(resource.path, { updateStatus: "checking", updateError: "" });
		try {
			const latest = await fetchLatestManifest(resource);
			const localVersion = normalizeVersion(resource.version ?? "");
			const latestVersion = normalizeVersion(latest.version);
			patchResource(resource.path, {
				updateStatus: localVersion && latestVersion && localVersion === latestVersion ? "up-to-date" : "update-available",
				latestVersion: latest.version,
				defaultBranch: latest.defaultBranch,
				latestManifestUrl: latest.manifestUrl,
				repositoryWebUrl: latest.webUrl,
				updateError: latest.version ? "" : "Remote manifest does not include a version value.",
			});
		} catch (caught) {
			patchResource(resource.path, {
				updateStatus: "error",
				updateError: caught instanceof Error ? caught.message : String(caught),
			});
		}
	}

	async function updateResource(resource: ResourceView, force = false) {
		if (!resource.repository) return;
		let target = resource;

		if (!target.defaultBranch) {
			await checkResourceUpdate(resource);
			target = resources.find((entry) => entry.path === resource.path) ?? resource;
		}

		if (!target.defaultBranch) {
			error = `Could not resolve the default branch for ${resource.name}.`;
			return;
		}

		const actionLabel = force ? "Reinstall" : "Update";
		const confirmed = window.confirm(`${actionLabel} ${resource.name} from ${target.repositoryWebUrl || resource.repository}? Existing files with the same names will be overwritten.`);
		if (!confirmed) return;

		patchResource(resource.path, { updating: true });
		error = "";
		message = "";
		try {
			await updateGithubResource({
				resourcePath: resource.path,
				repository: resource.repository,
				branch: target.defaultBranch,
			});
			message = force ? `${resource.name} reinstalled from GitHub.` : `${resource.name} updated from GitHub.`;
			await scanResources(false);
			const refreshed = resources.find((entry) => entry.path === resource.path);
			if (refreshed) await checkResourceUpdate(refreshed);
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		} finally {
			patchResource(resource.path, { updating: false });
		}
	}

	function openResourceContextMenu(event: MouseEvent, resource: ResourceView) {
		event.preventDefault();
		const menuWidth = 360;
		const menuHeight = 340;
		const padding = 12;
		contextMenuResource = resource;
		contextMenuPosition = {
			x: Math.min(event.clientX, Math.max(padding, window.innerWidth - menuWidth - padding)),
			y: Math.min(event.clientY, Math.max(padding, window.innerHeight - menuHeight - padding)),
		};
	}

	function closeResourceContextMenu() {
		contextMenuResource = null;
	}

	async function runContextCommand(action: "start" | "stop" | "restart" | "ensure" | "refresh", resource: ResourceView) {
		closeResourceContextMenu();
		await runResourceCommand(action, resource);
	}

	async function runContextUpdate(resource: ResourceView, force = false) {
		closeResourceContextMenu();
		await updateResource(resource, force);
	}

	async function runContextCheck(resource: ResourceView) {
		closeResourceContextMenu();
		await checkResourceUpdate(resource);
	}

	async function refreshRuntimeStates() {
		if (!resources.length) return;
		if (!rcon.password.trim()) {
			error = "Enter the RCON password before checking running resources.";
			return;
		}

		refreshingStates = true;
		error = "";
		message = "";
		try {
			await saveFxserverRconPassword(rcon.password);
			const result = await queryFxserverResourceStates(
				rcon,
				resources.map((resource) => resource.name),
			);
			const states = new Map(result.states.map((entry) => [entry.name, entry.state]));
			resources = resources.map((resource) => ({
				...resource,
				runtimeState: states.get(resource.name) ?? "unknown",
			}));
			message = result.rawOutput.trim()
				? "Resource state check completed. Some states may stay unknown if FXServer did not return a parseable resource list."
				: "RCON accepted the state check, but did not return a resource list.";
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		} finally {
			refreshingStates = false;
		}
	}

	async function runResourceCommand(action: "start" | "stop" | "restart" | "ensure" | "refresh", resource: ResourceView) {
		const command = action === "refresh" ? `refresh\nensure ${resource.name}` : `${action} ${resource.name}`;
		busyCommand = `${action}:${resource.path}`;
		error = "";
		message = "";
		try {
			if (rcon.password.trim()) await saveFxserverRconPassword(rcon.password);
			await sendFxserverRconCommand(command, rcon);
			recentCommands = [command, ...recentCommands].slice(0, 8);
			if (action === "stop") patchResource(resource.path, { runtimeState: "stopped" });
			if (action === "start" || action === "ensure" || action === "restart" || action === "refresh") patchResource(resource.path, { runtimeState: "running" });
			message = `Sent RCON command: ${command.replace("\n", " then ")}`;
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		} finally {
			busyCommand = "";
		}
	}

	async function clearPassword() {
		rcon = { ...rcon, password: "" };
		await clearFxserverRconPassword();
		message = "Saved RCON password cleared.";
	}

	function patchResource(path: string, patch: Partial<ResourceView>) {
		resources = resources.map((resource) => (resource.path === path ? { ...resource, ...patch } : resource));
	}

	async function fetchLatestManifest(resource: ResourceView) {
		const parsed = parseGithubRepository(resource.repository ?? "");
		if (!parsed) throw new Error("Repository is not a supported GitHub URL.");

		const repoResponse = await fetch(`https://api.github.com/repos/${parsed.owner}/${parsed.repo}`, {
			headers: { Accept: "application/vnd.github+json" },
		});
		if (!repoResponse.ok) throw new Error(`GitHub returned ${repoResponse.status} while loading repository metadata.`);

		const repoData = (await repoResponse.json()) as { default_branch?: string; html_url?: string };
		const defaultBranch = repoData.default_branch || "main";
		const manifestNames = [...new Set([resource.manifestName, "fxmanifest.lua", "__resource.lua"])];

		for (const manifestName of manifestNames) {
			const manifestUrl = `https://raw.githubusercontent.com/${parsed.owner}/${parsed.repo}/${encodeURIComponent(defaultBranch)}/${manifestName}`;
			const manifestResponse = await fetch(manifestUrl);
			if (!manifestResponse.ok) continue;

			const manifest = await manifestResponse.text();
			return {
				version: parseManifestValue(manifest, "version") ?? "",
				defaultBranch,
				manifestUrl,
				webUrl: repoData.html_url || `https://github.com/${parsed.owner}/${parsed.repo}`,
			};
		}

		throw new Error("No fxmanifest.lua or __resource.lua was found in the repository root.");
	}

	function parseGithubRepository(repository: string) {
		const cleaned = repository.trim().replace(/\.git$/, "").replace(/\/$/, "");
		const sshMatch = cleaned.match(/^git@github\.com:([^/]+)\/(.+)$/i);
		const urlMatch = cleaned.match(/github\.com\/([^/]+)\/([^/#?]+)/i);
		const match = sshMatch || urlMatch;
		if (!match) return null;
		return {
			owner: match[1],
			repo: match[2].replace(/\.git$/, ""),
		};
	}

	function normalizeRepositoryWebUrl(repository: string) {
		const parsed = parseGithubRepository(repository);
		return parsed ? `https://github.com/${parsed.owner}/${parsed.repo}` : repository;
	}

	function parseManifestValue(content: string, key: string) {
		for (const rawLine of content.split(/\r?\n/)) {
			const line = rawLine.trimStart();
			if (line.startsWith("--") || line.startsWith("#") || !line.startsWith(key)) continue;
			const remainder = line.slice(key.length);
			if (/^[A-Za-z0-9_]/.test(remainder)) continue;
			const match = remainder.match(/['"]([^'"]+)['"]/);
			if (match?.[1]?.trim()) return match[1].trim();
		}
		return "";
	}

	function normalizeVersion(version: string) {
		return version.trim().replace(/^v/i, "");
	}

	function updateBadgeClass(status: UpdateStatus) {
		return (
			{
				"up-to-date": "border-emerald-400/30 bg-emerald-400/10 text-emerald-200",
				"update-available": "border-amber-400/30 bg-amber-400/10 text-amber-100",
				checking: "border-sky-400/30 bg-sky-400/10 text-sky-200",
				"no-repository": "border-border bg-muted/30 text-muted-foreground",
				error: "border-red-400/30 bg-red-400/10 text-red-100",
				unchecked: "border-border bg-background/70 text-muted-foreground",
			} satisfies Record<UpdateStatus, string>
		)[status];
	}

	function updateLabel(resource: ResourceView) {
		if (resource.updateStatus === "up-to-date") return "Latest";
		if (resource.updateStatus === "update-available") return "Update available";
		if (resource.updateStatus === "checking") return "Checking";
		if (resource.updateStatus === "no-repository") return "Repository not found";
		if (resource.updateStatus === "error") return "Check failed";
		return "Not checked";
	}

	function runtimeBadgeClass(state: RuntimeState) {
		if (state === "running") return "border-emerald-400/30 bg-emerald-400/10 text-emerald-200";
		if (state === "stopped") return "border-red-400/30 bg-red-400/10 text-red-100";
		if (state === "missing") return "border-amber-400/30 bg-amber-400/10 text-amber-100";
		return "border-border bg-background/70 text-muted-foreground";
	}

	function isCommandBusy(action: string, resource: ResourceView) {
		return busyCommand === `${action}:${resource.path}`;
	}

	function displayResourcePath(path: string) {
		const normalizedPath = path.replace(/\\/g, "/");
		const normalizedDataPath = (scanResult?.dataPath ?? "").replace(/\\/g, "/").replace(/\/+$/, "");
		const dataPathName = normalizedDataPath.split("/").filter(Boolean).at(-1);
		if (!normalizedDataPath || !dataPathName) return normalizedPath;

		const dataPathWithSlash = `${normalizedDataPath}/`;
		if (normalizedPath.toLowerCase().startsWith(dataPathWithSlash.toLowerCase())) {
			return `${dataPathName}/${normalizedPath.slice(dataPathWithSlash.length)}`;
		}

		return normalizedPath.toLowerCase() === normalizedDataPath.toLowerCase() ? dataPathName : normalizedPath;
	}

	async function runLimited<T>(items: T[], limit: number, task: (item: T) => Promise<void>) {
		let nextIndex = 0;
		const workerCount = Math.min(limit, items.length);
		const workers = Array.from({ length: workerCount }, async () => {
			while (nextIndex < items.length) {
				const item = items[nextIndex++];
				await task(item);
			}
		});
		await Promise.all(workers);
	}
</script>

<svelte:window onclick={closeResourceContextMenu} onkeydown={(event) => event.key === "Escape" && closeResourceContextMenu()} />

<section class="space-y-6">
	<div class="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">FXServer</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Resource Manager</h1>
			<p class="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">Scan fxmanifest files, compare GitHub-backed resources, update resources, and control them through RCON.</p>
		</div>
		<div class="inline-flex items-center gap-2 rounded-sm border border-border bg-card px-3 py-2 text-xs text-muted-foreground">
			{#if scanning || checkingAll || refreshingStates || busyCommand}
				<RefreshCwIcon class="size-3.5 animate-spin" />
			{/if}
			{scanning || checkingAll || refreshingStates || busyCommand ? "Working..." : `${resources.length} resources`}
		</div>
	</div>

	{#if message}<Notice tone="success" {message} onDismiss={() => (message = "")} />{/if}
	{#if error}<Notice tone="error" message={error} onDismiss={() => (error = "")} />{/if}

	<div class="grid gap-4 xl:grid-cols-[minmax(0,1.1fr)_minmax(0,0.9fr)]">
		<Card.Root class="rounded-md border-border bg-card shadow-sm">
			<Card.Header class="border-b border-border pb-4">
				<div class="flex items-center gap-3">
					<div class="flex size-9 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
						<GitBranchIcon class="size-4" />
					</div>
					<div>
						<Card.Title>Resource Inventory</Card.Title>
						<Card.Description>Uses the selected txData profile and scans `dataPath/resources` for manifests.</Card.Description>
					</div>
				</div>
			</Card.Header>
			<Card.Content class="space-y-4">
				<div class="grid gap-3 md:grid-cols-3">
					<div class="rounded-sm border border-border bg-background/70 p-3">
						<p class="text-xs text-muted-foreground">Profile</p>
						<p class="mt-1 truncate font-mono text-sm text-foreground">{fxserverSettings.profile || "Not selected"}</p>
					</div>
					<div class="rounded-sm border border-border bg-background/70 p-3">
						<p class="text-xs text-muted-foreground">With Repository</p>
						<p class="mt-1 font-mono text-sm text-foreground">{resourcesWithRepositories}</p>
					</div>
					<div class="rounded-sm border border-border bg-background/70 p-3">
						<p class="text-xs text-muted-foreground">Updates</p>
						<p class="mt-1 font-mono text-sm text-foreground">{updateCount}</p>
					</div>
				</div>
				{#if scanResult}
					<p class="truncate rounded-sm border border-border bg-background/70 px-3 py-2 font-mono text-xs text-muted-foreground">{scanResult.resourceRoot}</p>
				{/if}
				<div class="flex flex-wrap gap-2">
					<Button onclick={() => scanResources()} disabled={scanning} title="Scan resources from the selected txData profile">
						<RefreshCwIcon class={scanning ? "animate-spin" : undefined} />
						Scan
					</Button>
					<Button variant="outline" onclick={checkAllUpdates} disabled={checkingAll || !resourcesWithRepositories} title="Check all resources with a repository entry">
						<GitBranchIcon class={checkingAll ? "animate-spin" : undefined} />
						Check Updates
					</Button>
					<Button variant="outline" onclick={refreshRuntimeStates} disabled={refreshingStates || !resources.length || !rcon.password.trim()} title="Best-effort RCON resource state check">
						<ActivityIcon class={refreshingStates ? "animate-spin" : undefined} />
						Runtime States
					</Button>
				</div>
			</Card.Content>
		</Card.Root>

		<Card.Root class="rounded-md border-border bg-card shadow-sm">
			<Card.Header class="border-b border-border pb-4">
				<Card.Title>RCON Connection</Card.Title>
				<Card.Description>Controls work against any reachable FXServer with a valid `rcon_password`.</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				<div class="grid gap-3 sm:grid-cols-[minmax(0,1fr)_8rem]">
					<label class="grid gap-2">
						<span class="text-xs font-medium text-muted-foreground">Host</span>
						<Input bind:value={rcon.host} placeholder="127.0.0.1" class="rounded-sm font-mono text-xs" />
					</label>
					<label class="grid gap-2">
						<span class="text-xs font-medium text-muted-foreground">Port</span>
						<Input type="number" bind:value={rcon.port} placeholder="30120" class="rounded-sm font-mono text-xs" />
					</label>
				</div>
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">RCON Password</span>
					<PasswordInput bind:value={rcon.password} placeholder="server.cfg rcon_password" class="rounded-sm font-mono text-xs" />
				</label>
				<div class="flex flex-wrap items-center gap-2">
					<Button variant="outline" onclick={clearPassword} disabled={!rcon.password} title="Clear saved RCON password">Clear Saved Password</Button>
					<div class="rounded-sm border border-border bg-background/70 px-3 py-2 text-xs text-muted-foreground">{runningCount} running detected</div>
				</div>
			</Card.Content>
		</Card.Root>
	</div>

	<Card.Root class="rounded-md border-border bg-card shadow-sm">
		<Card.Header class="border-b border-border pb-4">
			<div class="flex flex-col justify-between gap-3 lg:flex-row lg:items-center">
				<div>
					<Card.Title>Resources</Card.Title>
					<Card.Description>Resources without a `repository` entry can still be controlled, but update checks are marked as not found.</Card.Description>
				</div>
				<div class="flex flex-wrap gap-2">
					<Button variant={filter === "all" ? "default" : "outline"} size="sm" onclick={() => (filter = "all")}>All</Button>
					<Button variant={filter === "updates" ? "default" : "outline"} size="sm" onclick={() => (filter = "updates")}>Updates</Button>
					<Button variant={filter === "repos" ? "default" : "outline"} size="sm" onclick={() => (filter = "repos")}>Repos</Button>
					<Button variant={filter === "missing-repo" ? "default" : "outline"} size="sm" onclick={() => (filter = "missing-repo")}>No Repo</Button>
					<Button variant={filter === "running" ? "default" : "outline"} size="sm" onclick={() => (filter = "running")}>Running</Button>
					<Button variant={filter === "unknown" ? "default" : "outline"} size="sm" onclick={() => (filter = "unknown")}>Unknown</Button>
				</div>
			</div>
		</Card.Header>
		<Card.Content class="space-y-4">
			<div class="relative">
				<SearchIcon class="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
				<Input bind:value={search} placeholder="Search resources, versions, repositories..." class="rounded-sm pl-9" />
			</div>

			{#if filteredResources.length}
				<div class="grid gap-3">
					{#each filteredResources as resource (resource.path)}
						<article class="rounded-md border border-border bg-background/60 p-4 shadow-xs transition-colors hover:bg-background/80" oncontextmenu={(event) => openResourceContextMenu(event, resource)}>
							<div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
								<div class="min-w-0">
									<div class="flex flex-wrap items-center gap-2">
										<p class="text-base font-semibold text-foreground">{resource.name}</p>
										<span class={`rounded-sm border px-2 py-0.5 text-[10px] font-semibold uppercase ${runtimeBadgeClass(resource.runtimeState)}`}>{resource.runtimeState}</span>
										<span class={`rounded-sm border px-2 py-0.5 text-[10px] font-semibold uppercase ${updateBadgeClass(resource.updateStatus)}`}>{updateLabel(resource)}</span>
									</div>
								</div>
								<div class="grid grid-cols-2 gap-2 text-xs sm:w-80 sm:max-w-full">
									<div class="rounded-sm border border-border bg-card/45 px-3 py-2">
										<p class="text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">Local</p>
										<p class="mt-1 truncate font-mono text-sm text-foreground">{resource.version || "Unknown"}</p>
									</div>
									<div class="rounded-sm border border-border bg-card/45 px-3 py-2">
										<p class="text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">Latest</p>
										<p class="mt-1 truncate font-mono text-sm text-foreground">{resource.latestVersion || "Not checked"}</p>
									</div>
								</div>
							</div>

							{#if resource.updateError}
								<p class="mt-3 rounded-sm border border-red-400/20 bg-red-400/10 px-3 py-2 text-xs text-red-100">{resource.updateError}</p>
							{/if}

							<div class="mt-4 rounded-sm border border-border bg-card/45 p-3">
								<div class="grid gap-3 lg:grid-cols-2">
									<div class="min-w-0">
										<p class="text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">Local File Path</p>
										<p class="mt-2 break-all font-mono text-xs text-muted-foreground" title={resource.path}>{displayResourcePath(resource.path)}</p>
									</div>
									<div class="min-w-0">
										<p class="text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">Repository Source</p>
										{#if resource.repository}
											<button
												class="mt-2 flex max-w-full min-w-0 items-start gap-2 rounded-sm text-left font-mono text-xs text-sky-200 transition-colors hover:text-sky-100"
												onclick={() => openExternalUrl(resource.repositoryWebUrl || resource.repository || "")}
												title="Open repository"
											>
												<span class="min-w-0 break-all">{resource.repositoryWebUrl || resource.repository}</span>
												<ExternalLinkIcon class="mt-0.5 size-3 shrink-0" />
											</button>
										{:else}
											<p class="mt-2 font-mono text-xs text-muted-foreground">Repository not found in fxmanifest.</p>
										{/if}
									</div>
								</div>
								{#if resource.latestManifestUrl}
									<p class="mt-3 break-all border-t border-border pt-3 font-mono text-[11px] text-muted-foreground" title={resource.latestManifestUrl}>{resource.latestManifestUrl}</p>
								{/if}
							</div>

							<div class="mt-4 flex flex-col gap-3 border-t border-border pt-4 xl:flex-row xl:items-center xl:justify-between">
								<div class="min-w-0">
									<p class="mb-2 text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">Runtime Controls</p>
									<div class="flex flex-wrap gap-2">
										<Button size="xs" onclick={() => runResourceCommand("start", resource)} disabled={!rcon.password.trim() || Boolean(busyCommand)} title="Start resource">
											<PlayIcon class={isCommandBusy("start", resource) ? "animate-spin" : undefined} />Start
										</Button>
										<Button size="xs" variant="destructive" onclick={() => runResourceCommand("stop", resource)} disabled={!rcon.password.trim() || Boolean(busyCommand)} title="Stop resource">
											<SquareIcon class={isCommandBusy("stop", resource) ? "animate-spin" : undefined} />Stop
										</Button>
										<Button size="xs" variant="outline" onclick={() => runResourceCommand("restart", resource)} disabled={!rcon.password.trim() || Boolean(busyCommand)} title="Restart resource">
											<RotateCcwIcon class={isCommandBusy("restart", resource) ? "animate-spin" : undefined} />Restart
										</Button>
										<Button size="xs" variant="outline" onclick={() => runResourceCommand("ensure", resource)} disabled={!rcon.password.trim() || Boolean(busyCommand)} title="Ensure resource">
											<ShieldIcon class={isCommandBusy("ensure", resource) ? "animate-spin" : undefined} />Ensure
										</Button>
									</div>
								</div>
								<div class="min-w-0">
									<p class="mb-2 text-[10px] font-semibold tracking-wide text-muted-foreground uppercase xl:text-right">Update Controls</p>
									<div class="flex flex-wrap gap-2 xl:justify-end">
										<Button size="xs" variant="outline" onclick={() => checkResourceUpdate(resource)} disabled={!resource.repository || resource.updateStatus === "checking"} title="Check this resource against GitHub">
											<GitBranchIcon class={resource.updateStatus === "checking" ? "animate-spin" : undefined} />Check
										</Button>
										<Button size="xs" variant="outline" onclick={() => updateResource(resource)} disabled={!resource.repository || resource.updating || resource.updateStatus !== "update-available"} title="Update this resource from GitHub">
											<DownloadIcon class={resource.updating ? "animate-spin" : undefined} />Update
										</Button>
										<Button size="xs" variant="outline" onclick={() => updateResource(resource, true)} disabled={!resource.repository || resource.updating} title="Force reinstall this resource from GitHub">
											<DownloadIcon class={resource.updating ? "animate-spin" : undefined} />Reinstall
										</Button>
									</div>
								</div>
							</div>
						</article>
					{/each}
				</div>
			{:else}
				<div class="rounded-sm border border-dashed border-border bg-background/60 p-8 text-center text-sm text-muted-foreground">
					{resources.length ? "No resources match the current filter." : "No resources scanned yet."}
				</div>
			{/if}
		</Card.Content>
	</Card.Root>

	{#if contextMenuResource}
		<div
			class="fixed z-[110] w-[min(22rem,calc(100vw-1.5rem))] rounded-md border border-border bg-popover p-3 text-popover-foreground shadow-2xl animate-in fade-in-0 zoom-in-95 duration-100"
			style={`left: ${contextMenuPosition.x}px; top: ${contextMenuPosition.y}px;`}
			role="menu"
			tabindex="-1"
			onclick={(event) => event.stopPropagation()}
			onkeydown={(event) => {
				event.stopPropagation();
				if (event.key === "Escape") closeResourceContextMenu();
			}}
		>
			<div class="border-b border-border pb-3">
				<p class="truncate text-sm font-semibold text-foreground">{contextMenuResource.name}</p>
				<p class="mt-1 truncate font-mono text-xs text-muted-foreground">{displayResourcePath(contextMenuResource.path)}</p>
			</div>
			<div class="grid gap-4 pt-3">
				<div>
					<p class="mb-2 text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">Runtime Controls</p>
					<div class="grid grid-cols-2 gap-2">
						<Button size="lg" onclick={() => runContextCommand("start", contextMenuResource!)} disabled={!rcon.password.trim() || Boolean(busyCommand)} title="Start resource">
							<PlayIcon />Start
						</Button>
						<Button size="lg" variant="destructive" onclick={() => runContextCommand("stop", contextMenuResource!)} disabled={!rcon.password.trim() || Boolean(busyCommand)} title="Stop resource">
							<SquareIcon />Stop
						</Button>
						<Button size="lg" variant="outline" onclick={() => runContextCommand("restart", contextMenuResource!)} disabled={!rcon.password.trim() || Boolean(busyCommand)} title="Restart resource">
							<RotateCcwIcon />Restart
						</Button>
						<Button size="lg" variant="outline" onclick={() => runContextCommand("ensure", contextMenuResource!)} disabled={!rcon.password.trim() || Boolean(busyCommand)} title="Ensure resource">
							<ShieldIcon />Ensure
						</Button>
					</div>
				</div>
				<div>
					<p class="mb-2 text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">Update Controls</p>
					<div class="grid grid-cols-3 gap-2">
						<Button size="lg" variant="outline" onclick={() => runContextCheck(contextMenuResource!)} disabled={!contextMenuResource.repository || contextMenuResource.updateStatus === "checking"} title="Check this resource against GitHub">
							<GitBranchIcon />Check
						</Button>
						<Button size="lg" variant="outline" onclick={() => runContextUpdate(contextMenuResource!)} disabled={!contextMenuResource.repository || contextMenuResource.updating || contextMenuResource.updateStatus !== "update-available"} title="Update this resource from GitHub">
							<DownloadIcon />Update
						</Button>
						<Button size="lg" variant="outline" onclick={() => runContextUpdate(contextMenuResource!, true)} disabled={!contextMenuResource.repository || contextMenuResource.updating} title="Force reinstall this resource from GitHub">
							<DownloadIcon />Reinstall
						</Button>
					</div>
				</div>
			</div>
		</div>
	{/if}

	<Card.Root class="rounded-md border-border bg-card shadow-sm">
		<Card.Header class="border-b border-border pb-4">
			<Card.Title>Recent Commands</Card.Title>
			<Card.Description>Commands sent from this page during the current app session.</Card.Description>
		</Card.Header>
		<Card.Content>
			{#if recentCommands.length}
				<div class="grid gap-2">
					{#each recentCommands as command}
						<code class="rounded-sm border border-border bg-background/70 px-3 py-2 font-mono text-xs text-foreground">{command}</code>
					{/each}
				</div>
			{:else}
				<p class="text-sm text-muted-foreground">No resource commands sent yet.</p>
			{/if}
		</Card.Content>
	</Card.Root>
</section>
