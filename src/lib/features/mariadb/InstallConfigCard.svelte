<script lang="ts">
	import DownloadIcon from "@lucide/svelte/icons/download";
	import SlidersHorizontalIcon from "@lucide/svelte/icons/sliders-horizontal";
	import * as Card from "$lib/components/ui/card/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import PasswordInput from "$lib/components/ui/password-input.svelte";
	import { onMount } from "svelte";
	import { DEFAULT_MARIADB_SERIES, listMariaDBSeries, listMariaDBReleases, type MariaDBSeries, type MariaDBRelease, type MariaDBInstallOptions, type MariaDBPackageInfo } from "$lib/modules/mariadb";

	type Props = {
		busy: boolean;
		packageInfo: MariaDBPackageInfo | null;
		installStage: string;
		installOptions: MariaDBInstallOptions;
		onInstall: () => void;
	};

	let { busy, packageInfo, installStage, installOptions = $bindable(), onInstall }: Props = $props();
	let series = $state(installOptions.version?.split(".").slice(0, 2).join(".") || DEFAULT_MARIADB_SERIES);
	let seriesList = $state<MariaDBSeries[]>([]);
	let releases = $state<MariaDBRelease[]>([]);
	let loadingVersions = $state(true);
	let versionsError = $state("");
	let preset = $state("recommended");
	let requestId = 0;
	let intendedVersion = $derived(installOptions.version?.split(".").length === 3 ? installOptions.version : releases[0]?.version);
	let validSelection = $derived(!!intendedVersion && releases.some((release) => release.version === intendedVersion));
	const presetItems = [
		{ value: "recommended", label: "Recommended LTS (11.4)" },
		{ value: "legacy", label: "Legacy application (10.11 LTS)" },
		{ value: "custom", label: "Custom" },
	];
	let seriesItems = $derived(seriesList.length ? seriesList.map((item) => ({ value: item.series, label: `${item.series} (until ${item.eol})` })) : [{ value: series, label: series }]);
	let releaseItems = $derived([
		{ value: series, label: `Latest in ${series}${releases[0] ? ` (${releases[0].version})` : ""}` },
		...releases.map((release) => ({ value: release.version, label: `${release.version} (${release.date})` })),
	]);

	onMount(() => { void loadVersions(true); return () => { requestId += 1; }; });

	async function loadVersions(loadSeries = false) {
		const id = ++requestId;
		const selectedSeries = series;
		loadingVersions = true;
		versionsError = "";
		releases = [];
		try {
			if (loadSeries) {
				const values = await listMariaDBSeries();
				if (id !== requestId) return;
				seriesList = values;
			}
			const values = await listMariaDBReleases(selectedSeries);
			if (id !== requestId) return;
			releases = values;
			if (!values.length) versionsError = "No supported Windows x64 releases are available for this series.";
		} catch (caught) {
			if (id === requestId) versionsError = String(caught);
		} finally {
			if (id === requestId) loadingVersions = false;
		}
	}

	function chooseSeries(value: string) {
		series = value;
		installOptions.version = value;
		void loadVersions();
	}

	function applyPreset(value: string) {
		preset = value;
		if (value === "custom") return;
		installOptions.allowRemoteRootAccess = false;
		installOptions.createAnonymousUser = false;
		installOptions.skipNetworking = false;
		installOptions.optimizeForTransactions = true;
		installOptions.useUtf8 = true;
		installOptions.pageSize = "";
		installOptions.bufferPoolSize = "";
		chooseSeries(value === "legacy" ? "10.11" : DEFAULT_MARIADB_SERIES);
	}

	const boolOptions = [
		["allowRemoteRootAccess", "Remote root", "Allow the MariaDB root account to connect from remote hosts."],
		["createAnonymousUser", "Anonymous user", "Create the default anonymous database user during installation."],
		["skipNetworking", "Skip networking", "Disable TCP networking and only allow local socket or pipe access."],
		["optimizeForTransactions", "Optimize", "Apply MariaDB's standard transactional configuration preset."],
		["useUtf8", "UTF-8", "Use UTF-8 as the default server character set."],
		["installHeidiSql", "HeidiSQL", "Install the bundled HeidiSQL database administration tool."],
		["installDevelopmentFiles", "Dev files", "Install MariaDB development headers and libraries."],
	] as const;
</script>

<Card.Root class="h-full rounded-md border-border bg-card shadow-sm">
	<Card.Header class="border-b border-border pb-4">
		<div class="flex items-center justify-between gap-3">
			<div class="flex min-w-0 items-center gap-3">
				<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
					<SlidersHorizontalIcon class="size-4" />
				</div>
				<div class="min-w-0">
					<Card.Title>Install Configuration</Card.Title>
					<Card.Description>
						MariaDB {intendedVersion || `${series} series`} - official Windows x64 MSI.
					</Card.Description>
				</div>
			</div>
			<Button onclick={() => { if (intendedVersion && validSelection) { installOptions.version = intendedVersion; onInstall(); } }} disabled={busy || loadingVersions || !validSelection} title="Install the displayed MariaDB release">
				<DownloadIcon />
				Install
			</Button>
		</div>
	</Card.Header>

	<Card.Content class="space-y-4">
		<div class="grid gap-3 md:grid-cols-3">
			<div class="grid min-w-0 gap-1.5 text-sm">
				<label for="mariadb-preset">Preset</label>
				<Select.Root type="single" value={preset} items={presetItems} disabled={busy || loadingVersions} onValueChange={applyPreset}>
					<Select.Trigger id="mariadb-preset" aria-label="Preset" class="w-full min-w-0"><span class="truncate">{presetItems.find((item) => item.value === preset)?.label}</span></Select.Trigger>
					<Select.Content>{#each presetItems as item}<Select.Item value={item.value} label={item.label}>{item.label}</Select.Item>{/each}</Select.Content>
				</Select.Root>
			</div>
			<div class="grid min-w-0 gap-1.5 text-sm">
				<label for="mariadb-series">Supported LTS Series</label>
				<Select.Root type="single" value={series} items={seriesItems} disabled={busy || loadingVersions} onValueChange={(value) => { preset = "custom"; chooseSeries(value); }}>
					<Select.Trigger id="mariadb-series" aria-label="Supported LTS Series" class="w-full min-w-0"><span class="truncate">{seriesItems.find((item) => item.value === series)?.label || series}</span></Select.Trigger>
					<Select.Content>{#each seriesItems as item}<Select.Item value={item.value} label={item.label}>{item.label}</Select.Item>{/each}</Select.Content>
				</Select.Root>
			</div>
			<div class="grid min-w-0 gap-1.5 text-sm">
				<label for="mariadb-release">Release</label>
				<Select.Root type="single" value={installOptions.version || series} items={releaseItems} disabled={busy || loadingVersions} onValueChange={(value) => { preset = "custom"; installOptions.version = value; }}>
					<Select.Trigger id="mariadb-release" aria-label="Release" class="w-full min-w-0"><span class="truncate">{releaseItems.find((item) => item.value === (installOptions.version || series))?.label || installOptions.version}</span></Select.Trigger>
					<Select.Content>{#each releaseItems as item}<Select.Item value={item.value} label={item.label}>{item.label}</Select.Item>{/each}</Select.Content>
				</Select.Root>
			</div>
		</div>
		{#if versionsError}
			<p class="text-sm text-destructive" role="alert">{versionsError}</p>
			<Button variant="outline" disabled={busy || loadingVersions} onclick={() => loadVersions(true)}>Retry version lookup</Button>
		{/if}
		<div class="grid gap-3 md:grid-cols-3">
			<div class="rounded-sm border border-border bg-background px-3 py-2">
				<p class="text-xs text-muted-foreground">Selected Release</p>
				<p class="mt-1 font-semibold">{loadingVersions ? "Checking..." : intendedVersion || "Unavailable"}</p>
			</div>
			<div class="rounded-sm border border-border bg-background px-3 py-2 md:col-span-2">
				<p class="text-xs text-muted-foreground">Install Progress</p>
				<p class="mt-1 text-sm font-medium">{installStage || "Ready to install."}</p>
			</div>
		</div>

		{#if busy && installStage}
			<p class="rounded-sm border border-primary/30 bg-primary/10 px-3 py-2 text-xs text-primary">
				If Windows asks for administrator permission, press Yes to allow the MariaDB MSI to register the service.
			</p>
		{/if}

		<div class="grid gap-3 md:grid-cols-3">
			<label class="grid gap-1.5">
				<span class="text-xs font-medium text-muted-foreground">Root Password</span>
				<PasswordInput bind:value={installOptions.rootPassword} placeholder="Required root password" title="Root password used by the MariaDB installer." />
			</label>
			<label class="grid gap-1.5">
				<span class="text-xs font-medium text-muted-foreground">Service Name</span>
				<Input bind:value={installOptions.serviceName} placeholder="MariaDB" title="Windows service name to register for MariaDB." />
			</label>
			<label class="grid gap-1.5">
				<span class="text-xs font-medium text-muted-foreground">Port</span>
				<Input type="number" bind:value={installOptions.port} placeholder="3306" title="TCP port MariaDB should listen on." />
			</label>
			<label class="grid gap-1.5">
				<span class="text-xs font-medium text-muted-foreground">Install Directory</span>
				<Input bind:value={installOptions.installDir} placeholder="C:\\Program Files\\MariaDB" title="Optional MariaDB installation directory." />
			</label>
			<label class="grid gap-1.5">
				<span class="text-xs font-medium text-muted-foreground">Data Directory</span>
				<Input bind:value={installOptions.dataDir} placeholder="C:\\Program Files\\MariaDB\\data" title="Optional directory for MariaDB data files." />
			</label>
			<label class="grid gap-1.5">
				<span class="text-xs font-medium text-muted-foreground">Buffer Pool</span>
				<Input bind:value={installOptions.bufferPoolSize} placeholder="RAM/8, 512M, 1G" title="Optional InnoDB buffer pool size." />
			</label>
			<label class="grid gap-1.5">
				<span class="text-xs font-medium text-muted-foreground">Page Size</span>
				<select
					bind:value={installOptions.pageSize}
					title="Optional InnoDB page size."
					class="h-9 rounded-sm border border-input bg-background px-2.5 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
				>
					<option value="">Default</option>
					<option value="4K">4K</option>
					<option value="8K">8K</option>
					<option value="16K">16K</option>
					<option value="32K">32K</option>
					<option value="64K">64K</option>
				</select>
			</label>
		</div>

		<div class="grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
			{#each boolOptions as [key, label, description]}
				<label class="flex h-9 items-center gap-2 rounded-sm border border-border bg-background px-2.5 text-sm whitespace-nowrap" title={description}>
					<input
						type="checkbox"
						bind:checked={installOptions[key]}
						class="size-3.5 rounded-xs border-border bg-background accent-foreground"
						title={description}
					/>
					<span>{label}</span>
				</label>
			{/each}
		</div>

		{#if installOptions.skipNetworking}
			<p class="rounded-sm border border-amber-400/30 bg-amber-400/10 px-3 py-2 text-xs text-amber-100">
				Skip networking disables TCP/IP. The service can install and run, but this app's MariaDB connection tools expect localhost TCP access.
			</p>
		{/if}

		<p class="rounded-sm border border-border bg-background/70 px-3 py-2 text-xs text-muted-foreground">
			Preserved databases require a known version in the same series and an equal or newer server. Existing passwords remain unchanged. Cross-series changes require a backed-up migration.
		</p>
	</Card.Content>
</Card.Root>
