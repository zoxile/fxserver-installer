<script lang="ts">
	import AlertTriangleIcon from "@lucide/svelte/icons/alert-triangle";
	import CheckCircle2Icon from "@lucide/svelte/icons/check-circle-2";
	import ClipboardIcon from "@lucide/svelte/icons/clipboard";
	import DownloadIcon from "@lucide/svelte/icons/download";
	import FileCode2Icon from "@lucide/svelte/icons/file-code-2";
	import FolderTreeIcon from "@lucide/svelte/icons/folder-tree";
	import PlusIcon from "@lucide/svelte/icons/plus";
	import RotateCcwIcon from "@lucide/svelte/icons/rotate-ccw";
	import SearchIcon from "@lucide/svelte/icons/search";
	import SlidersHorizontalIcon from "@lucide/svelte/icons/sliders-horizontal";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import UploadCloudIcon from "@lucide/svelte/icons/upload-cloud";
	import WandSparklesIcon from "@lucide/svelte/icons/wand-sparkles";
	import { onMount } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { log } from "$lib/core/logger.svelte";
	import {
		addConfigEntry,
		configPathKey,
		getConfigSettings,
		getConfigObjects,
		getConfigWarnings,
		parseConfigLua,
		removeConfigEntry,
		sampleConfigLua,
		stringifyConfig,
		stringifyLuaValue,
		updateConfigValue,
		valueSummary,
		type ConfigSetting,
		type LuaValue,
		type LuaValueType,
	} from "./configLua";

	let source = $state(sampleConfigLua);
	let config = $state<LuaValue | null>(null);
	let selectedId = $state("");
	let search = $state("");
	let notice = $state<{ type: "success" | "error" | "warn"; message: string } | null>(null);
	let parseWarnings = $state<string[]>([]);
	let commentsByPath = $state<Record<string, string>>({});
	let unassignedComments = $state<string[]>([]);
	let baselineValues = $state<Record<string, string>>({});
	let selectedGroupId = $state("[]");
	let newFieldName = $state("");
	let newFieldType = $state<Exclude<LuaValueType, "nil" | "raw">>("string");
	let dragging = $state(false);
	let fileInput: HTMLInputElement;
	let sourceHighlight: HTMLPreElement;

	const sourcePlaceholder = "Config = {}\nConfig.Debug = true\nConfig.Spawn = vector3(0.0, 0.0, 0.0)";
	type LuaTokenType = "comment" | "string" | "number" | "boolean" | "nil" | "vector" | "config" | "keyword" | "punctuation" | "plain";
	const fieldTypes: Array<{ value: typeof newFieldType; label: string; hint: string }> = [
		{ value: "string", label: "String", hint: "text" },
		{ value: "number", label: "Number", hint: "numeric" },
		{ value: "boolean", label: "Boolean", hint: "true / false" },
		{ value: "vector2", label: "Vector2", hint: "x, y" },
		{ value: "vector3", label: "Vector3", hint: "x, y, z" },
		{ value: "vector4", label: "Vector4", hint: "x, y, z, w" },
		{ value: "table", label: "Object", hint: "keyed fields" },
		{ value: "array", label: "Array", hint: "ordered entries" },
	];
	const settings = $derived(config ? getConfigSettings(config, commentsByPath) : []);
	const groups = $derived(config ? getConfigObjects(config, commentsByPath) : []);
	const output = $derived(config ? stringifyConfig(config, commentsByPath) : "");
	const highlightedSource = $derived(highlightLuaCode(source));
	const highlightedOutput = $derived(highlightLuaCode(output));
	const warnings = $derived(config ? [...parseWarnings, ...getConfigWarnings(config)] : parseWarnings);
	const selected = $derived(settings.find((entry) => entry.id === selectedId) ?? settings[0] ?? null);
	const selectedPreviewTokens = $derived(selected ? highlightLuaCode(selectedPreview(selected)) : []);
	const selectedGroup = $derived(groups.find((group) => group.id === selectedGroupId) ?? groups[0] ?? null);
	const filteredSettings = $derived(
		settings.filter((entry) => {
			const needle = search.trim().toLowerCase();
			const parentMatches = selectedGroup ? configPathKey(entry.path.slice(0, -1)) === selectedGroup.id : true;
			if (!parentMatches) return false;
			if (!needle) return true;
			return `${displayLabel(entry)} ${entry.type} ${valueSummary(entry.value)} ${entry.comment ?? ""}`.toLowerCase().includes(needle);
		}),
	);
	const changedCount = $derived(settings.filter((entry) => baselineValues[entry.id] !== stringifyLuaValue(entry.value)).length);

	$effect(() => {
		if (!selectedId && settings.length) {
			selectedId = settings[0].id;
		}
		if (groups.length && !groups.some((group) => group.id === selectedGroupId)) {
			selectedGroupId = groups[0].id;
		}
	});

	onMount(() => {
		parseSource();
	});

	function parseSource() {
		try {
			const parsed = parseConfigLua(source);
			config = parsed.root;
			parseWarnings = parsed.warnings;
			commentsByPath = parsed.commentsByPath;
			unassignedComments = parsed.unassignedComments;
			selectedId = parsed.settings[0]?.id ?? "";
			selectedGroupId = "[]";
			baselineValues = Object.fromEntries(parsed.settings.map((entry) => [entry.id, stringifyLuaValue(entry.value)]));
			notice = { type: parsed.warnings.length ? "warn" : "success", message: `Parsed ${parsed.settings.length} editable config values.` };
			log("Lua configuration parsed in configurator.", { level: "success", scope: "configurator", detail: `${parsed.settings.length} settings, ${parsed.warnings.length} warnings` });
		} catch (error) {
			config = null;
			selectedId = "";
			parseWarnings = [];
			commentsByPath = {};
			unassignedComments = [];
			notice = { type: "error", message: error instanceof Error ? error.message : String(error) };
			log("Lua configuration parsing failed.", { level: "error", scope: "configurator", detail: notice.message });
		}
	}

	function loadSample() {
		source = sampleConfigLua;
		parseSource();
		log("Configurator sample config loaded.", { level: "debug", scope: "configurator" });
	}

	async function uploadFile(file?: File) {
		if (!file) return;

		if (!file.name.endsWith(".lua")) {
			notice = { type: "error", message: "Please upload a .lua file." };
			log("Configurator rejected a non-Lua file upload.", { level: "warn", scope: "configurator", detail: file.name });
			return;
		}

		source = await file.text();
		parseSource();
		log("Lua configuration uploaded into configurator.", { level: "success", scope: "configurator", detail: `${file.name} (${source.length} characters)` });
	}

	function onDrop(event: DragEvent) {
		event.preventDefault();
		dragging = false;
		void uploadFile(event.dataTransfer?.files?.[0]);
	}

	function updateSelected(nextValue: LuaValue) {
		if (!config || !selected) return;
		config = updateConfigValue(config, selected.path, nextValue);
		log(`Configurator updated ${displayLabel(selected)}.`, { level: "debug", scope: "configurator", detail: nextValue.type });
	}

	function updateComment(event: Event) {
		if (!selected) return;
		const value = (event.currentTarget as HTMLTextAreaElement).value.trim();
		const key = configPathKey(selected.path);
		const next = { ...commentsByPath };
		if (value) {
			next[key] = value;
		} else {
			delete next[key];
		}
		commentsByPath = next;
	}

	function addField() {
		if (!config || !selectedGroup) return;

		try {
			config = addConfigEntry(config, selectedGroup.path, newFieldName, newFieldType);
			const nextPath = [...selectedGroup.path, /^\d+$/.test(newFieldName.trim()) ? Number(newFieldName.trim()) : newFieldName.trim()];
			selectedGroupId = configPathKey(selectedGroup.path);
			selectedId = settingIdFromPath(nextPath);
			newFieldName = "";
			notice = { type: "success", message: `Added field to ${selectedGroup.label}.` };
			log("Configurator field added.", { level: "success", scope: "configurator", detail: selectedId });
		} catch (error) {
			notice = { type: "error", message: error instanceof Error ? error.message : String(error) };
		}
	}

	function addObject() {
		if (!config || !selectedGroup) return;

		try {
			config = addConfigEntry(config, selectedGroup.path, newFieldName, "table");
			const nextPath = [...selectedGroup.path, /^\d+$/.test(newFieldName.trim()) ? Number(newFieldName.trim()) : newFieldName.trim()];
			selectedGroupId = configPathKey(nextPath);
			selectedId = "";
			newFieldName = "";
			notice = { type: "success", message: `Added object inside ${selectedGroup.label}.` };
			log("Configurator object added.", { level: "success", scope: "configurator", detail: settingIdFromPath(nextPath) });
		} catch (error) {
			notice = { type: "error", message: error instanceof Error ? error.message : String(error) };
		}
	}

	function removeSelected() {
		if (!config || !selected || !selected.path.length) return;
		const removedLabel = displayLabel(selected);
		config = removeConfigEntry(config, selected.path);
		const next = { ...commentsByPath };
		delete next[configPathKey(selected.path)];
		commentsByPath = next;
		selectedId = "";
		notice = { type: "warn", message: `Removed ${removedLabel}.` };
		log("Configurator field removed.", { level: "warn", scope: "configurator", detail: removedLabel });
	}

	function selectGroup(groupId: string) {
		selectedGroupId = groupId;
		const firstInGroup = settings.find((entry) => configPathKey(entry.path.slice(0, -1)) === groupId);
		if (firstInGroup) selectedId = firstInGroup.id;
	}

	function updateString(event: Event) {
		updateSelected({ type: "string", value: (event.currentTarget as HTMLInputElement).value });
	}

	function updateNumber(event: Event) {
		const value = Number((event.currentTarget as HTMLInputElement).value);
		if (!Number.isFinite(value)) {
			notice = { type: "error", message: "Number settings only accept finite numeric values." };
			return;
		}
		updateSelected({ type: "number", value });
	}

	function updateBoolean(value: boolean) {
		updateSelected({ type: "boolean", value });
	}

	function updateVector(index: number, event: Event) {
		if (!selected || (selected.value.type !== "vector2" && selected.value.type !== "vector3" && selected.value.type !== "vector4")) return;
		const next = Number((event.currentTarget as HTMLInputElement).value);
		if (!Number.isFinite(next)) {
			notice = { type: "error", message: "Vector components only accept finite numeric values." };
			return;
		}

		const values = [...selected.value.values];
		values[index] = next;
		updateSelected({ type: selected.value.type, values });
	}

	async function copyOutput() {
		if (!output) return;
		await navigator.clipboard.writeText(output);
		notice = { type: "success", message: "Generated Lua copied to clipboard." };
		log("Generated Lua configuration copied from configurator.", { level: "success", scope: "configurator" });
	}

	function downloadOutput() {
		if (!output) return;
		const blob = new Blob([output], { type: "text/plain;charset=utf-8" });
		const url = URL.createObjectURL(blob);
		const anchor = document.createElement("a");
		anchor.href = url;
		anchor.download = "configuration.lua";
		anchor.click();
		URL.revokeObjectURL(url);
		log("Generated Lua configuration downloaded from configurator.", { level: "success", scope: "configurator" });
	}

	function syncGeneratedToSource() {
		if (!output) return;
		source = output;
		parseSource();
		notice = { type: "success", message: "Generated Lua was moved back into the source editor." };
	}

	function displayLabel(setting: ConfigSetting) {
		return setting.label.replace(/^Config\./, "").replace(/^Config$/, "Root");
	}

	function syncSourceScroll(event: Event) {
		if (!sourceHighlight) return;
		const textarea = event.currentTarget as HTMLTextAreaElement;
		sourceHighlight.scrollTop = textarea.scrollTop;
		sourceHighlight.scrollLeft = textarea.scrollLeft;
	}

	function settingIdFromPath(path: Array<string | number>) {
		if (!path.length) return "Config";

		return `Config.${path
			.map((part) => {
				if (typeof part === "number") return `[${part}]`;
				return /^[A-Za-z_][A-Za-z0-9_]*$/.test(part) ? part : `["${part}"]`;
			})
			.join(".")
			.replace(/\.\[/g, "[")}`;
	}

	function parentLabel(setting: ConfigSetting) {
		const parentPath = setting.path.slice(0, -1);
		if (!parentPath.length) return "Configuration";
		return settingIdFromPath(parentPath).replace(/^Config\./, "");
	}

	function fieldName(setting: ConfigSetting) {
		const part = setting.path.at(-1);
		return part === undefined ? "Config" : String(part);
	}

	function selectedPreview(setting: ConfigSetting) {
		return `${setting.label} = ${stringifyLuaValue(setting.value)}`;
	}

	function selectedFieldTypeLabel() {
		return fieldTypes.find((type) => type.value === newFieldType)?.label ?? "Field type";
	}

	function typeClass(type: string) {
		return (
			{
				string: "border-sky-400/30 bg-sky-400/10 text-sky-200",
				number: "border-emerald-400/30 bg-emerald-400/10 text-emerald-200",
				boolean: "border-violet-400/30 bg-violet-400/10 text-violet-200",
				vector2: "border-amber-400/30 bg-amber-400/10 text-amber-200",
				vector3: "border-amber-400/30 bg-amber-400/10 text-amber-200",
				vector4: "border-amber-400/30 bg-amber-400/10 text-amber-200",
				table: "border-muted bg-muted/50 text-muted-foreground",
				array: "border-cyan-400/30 bg-cyan-400/10 text-cyan-200",
				nil: "border-muted bg-muted/50 text-muted-foreground",
				raw: "border-red-400/30 bg-red-400/10 text-red-200",
			}[type] ?? "border-border bg-muted text-muted-foreground"
		);
	}

	function booleanButtonClass(active: boolean, tone: "true" | "false") {
		const activeBase = "h-8 rounded-xs px-3 text-xs font-medium uppercase shadow-none focus-visible:ring-0";
		if (active && tone === "true")
			return `${activeBase} border-emerald-400/30 bg-emerald-400/10 text-emerald-200 hover:bg-emerald-400/10 hover:text-emerald-200 focus-visible:border-emerald-400/30 dark:border-emerald-400/30 dark:bg-emerald-400/10 dark:text-emerald-200 dark:hover:bg-emerald-400/10 dark:hover:text-emerald-200`;
		if (active && tone === "false")
			return `${activeBase} border-red-400/30 bg-red-400/10 text-red-200 hover:bg-red-400/10 hover:text-red-200 focus-visible:border-red-400/30 dark:border-red-400/30 dark:bg-red-400/10 dark:text-red-200 dark:hover:bg-red-400/10 dark:hover:text-red-200`;
		return "h-8 rounded-xs border-border bg-background/70 px-3 text-xs font-medium uppercase text-muted-foreground shadow-none hover:border-muted-foreground/40 hover:bg-muted/40 hover:text-foreground";
	}

	function tokenClass(type: LuaTokenType) {
		return {
			comment: "text-[#6A9955]",
			string: "text-[#CE9178]",
			number: "text-[#B5CEA8]",
			boolean: "text-[#569CD6]",
			nil: "text-[#569CD6]",
			vector: "text-[#DCDCAA]",
			config: "text-[#9CDCFE]",
			keyword: "text-[#C586C0]",
			punctuation: "text-[#808080]",
			plain: "text-[#D4D4D4]",
		}[type];
	}

	function highlightLuaCode(code: string) {
		const tokens: Array<{ text: string; type: LuaTokenType }> = [];
		const pattern =
			/(--\[\[[\s\S]*?\]\]|--[^\n]*|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|-?\b\d+\.?\d*(?:e[+-]?\d+)?\b|\b(?:true|false|nil|Config|return|local|function|end|if|then|else|elseif|for|while|do|in)\b|\bvector[234]\b|[{}\[\]().,;=])/gi;
		let cursor = 0;
		let match: RegExpExecArray | null;

		while ((match = pattern.exec(code))) {
			if (match.index > cursor) tokens.push({ text: code.slice(cursor, match.index), type: "plain" });
			const text = match[0];
			tokens.push({ text, type: classifyLuaToken(text) });
			cursor = match.index + text.length;
		}

		if (cursor < code.length) tokens.push({ text: code.slice(cursor), type: "plain" });
		return tokens.length ? tokens : [{ text: " ", type: "plain" as const }];
	}

	function classifyLuaToken(text: string): LuaTokenType {
		if (text.startsWith("--")) return "comment";
		if (text.startsWith('"') || text.startsWith("'")) return "string";
		if (/^-?\d/.test(text)) return "number";
		if (text === "true" || text === "false") return "boolean";
		if (text === "nil") return "nil";
		if (/^vector[234]$/i.test(text)) return "vector";
		if (text === "Config") return "config";
		if (/^(return|local|function|end|if|then|else|elseif|for|while|do|in)$/i.test(text)) return "keyword";
		if (/^[{}\[\]().,;=]$/.test(text)) return "punctuation";
		return "plain";
	}

	function noticeClass(type: "success" | "error" | "warn") {
		return {
			success: "border-emerald-400/30 bg-emerald-400/10 text-emerald-200",
			error: "border-red-400/30 bg-red-400/10 text-red-200",
			warn: "border-amber-400/30 bg-amber-400/10 text-amber-200",
		}[type];
	}
</script>

<section class="space-y-6">
	<div class="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Tools</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Configurator</h1>
			<p class="mt-2 max-w-2xl text-sm text-muted-foreground">Drop in a FiveM Lua configuration file, edit supported values through typed controls, and export clean Lua back out.</p>
		</div>
		<div class="inline-flex items-center gap-2 rounded-sm border border-border bg-card px-3 py-2 text-xs text-muted-foreground">
			<SlidersHorizontalIcon class="size-3.5" />
			{settings.length} settings
		</div>
	</div>

	{#if notice}
		<Notice tone={notice.type} message={notice.message} onDismiss={() => (notice = null)} class="text-sm" />
	{/if}

	<div class="grid items-stretch gap-4 xl:grid-cols-[minmax(0,1fr)_minmax(10rem,12rem)]">
		<Card.Root class="group relative flex max-h-184 flex-col overflow-hidden rounded-sm border-border bg-card shadow-sm transition-shadow duration-500 ease-[cubic-bezier(0.22,1,0.36,1)]">
			<div
				class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
			></div>
			<Card.Header class="border-b border-border pb-4">
				<div class="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
					<div class="flex items-start gap-3">
						<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
							<FileCode2Icon class="size-5" />
						</div>
						<div>
							<Card.Title>Source Config</Card.Title>
							<Card.Description>Paste Lua, upload a file, or drop a Lua configuration onto the editor.</Card.Description>
						</div>
					</div>
					<div class="grid shrink-0 grid-cols-3 gap-2">
						<input bind:this={fileInput} type="file" accept=".lua,text/plain" class="hidden" onchange={(event) => void uploadFile(event.currentTarget.files?.[0])} />
						<Button variant="outline" class="whitespace-nowrap" onclick={() => fileInput.click()} title="Upload a Lua configuration file">
							<UploadCloudIcon />
							Upload
						</Button>
						<Button variant="outline" class="whitespace-nowrap" onclick={loadSample} title="Load a sample FiveM config">
							<RotateCcwIcon />
							Sample
						</Button>
						<Button class="whitespace-nowrap" onclick={parseSource} title="Parse the source Lua into typed editor controls">
							<WandSparklesIcon />
							Parse
						</Button>
					</div>
				</div>
			</Card.Header>
			<Card.Content class="min-h-0 space-y-4 overflow-auto">
				<label
					class={["relative block overflow-hidden rounded-sm border bg-background/60 transition-colors", dragging ? "border-primary/60 bg-primary/10" : "border-border"]}
					ondragover={(event) => {
						event.preventDefault();
						dragging = true;
					}}
					ondragleave={() => (dragging = false)}
					ondrop={onDrop}
				>
					<pre
						bind:this={sourceHighlight}
						aria-hidden="true"
						class="pointer-events-none absolute inset-0 h-128 overflow-hidden px-3 py-3 font-mono text-xs leading-5 whitespace-pre text-[#D4D4D4]">{#each highlightedSource as token}<span
								class={tokenClass(token.type)}>{token.text}</span
							>{/each}</pre>
					<textarea
						bind:value={source}
						wrap="off"
						spellcheck="false"
						placeholder={sourcePlaceholder}
						title="Lua config source to parse into editable fields."
						onscroll={syncSourceScroll}
						class="relative h-128 w-full resize-none overflow-auto border-0 bg-transparent px-3 py-3 font-mono text-xs leading-5 whitespace-pre text-transparent caret-foreground outline-none selection:bg-primary/30 placeholder:text-muted-foreground"
					></textarea>
				</label>
			</Card.Content>
		</Card.Root>

		<div class="h-full">
			<div class="grid h-full gap-3 sm:grid-cols-3 xl:grid-cols-1 xl:grid-rows-3">
				{#each [{ label: "Parsed Settings", value: String(settings.length), description: "typed fields" }, { label: "Changed", value: String(changedCount), description: "since last parse" }, { label: "Warnings", value: String(warnings.length), description: "needs review" }] as stat}
					<Card.Root class="group relative h-full overflow-hidden rounded-sm border-border bg-card shadow-sm transition-shadow duration-500 ease-[cubic-bezier(0.22,1,0.36,1)]">
						<div
							class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
						></div>
						<Card.Content class="p-3">
							<p class="text-xs text-muted-foreground">{stat.label}</p>
							<p class="mt-1 text-xl font-semibold text-foreground">{stat.value}</p>
							<p class="mt-1 text-xs text-muted-foreground">{stat.description}</p>
						</Card.Content>
					</Card.Root>
				{/each}
			</div>
		</div>
	</div>

	{#if warnings.length || unassignedComments.length}
		<div class="grid gap-4 xl:grid-cols-2">
			{#if warnings.length}
				<Card.Root class="rounded-sm border-amber-400/30 bg-card shadow-sm">
					<Card.Header class="border-b border-border pb-4">
						<Card.Title>Parser Notes</Card.Title>
						<Card.Description>Unsupported expressions are preserved but not typed-editable.</Card.Description>
					</Card.Header>
					<Card.Content>
						<div class="max-h-44 space-y-2 overflow-auto text-xs text-amber-100">
							{#each warnings as warning}
								<p class="rounded-sm border border-amber-400/20 bg-amber-400/10 px-2 py-1">{warning}</p>
							{/each}
						</div>
					</Card.Content>
				</Card.Root>
			{/if}

			{#if unassignedComments.length}
				<Card.Root class="rounded-sm border-border bg-card shadow-sm">
					<Card.Header class="border-b border-border pb-4">
						<Card.Title>Other Comments</Card.Title>
						<Card.Description>Comments found in the file that were not close enough to a specific value.</Card.Description>
					</Card.Header>
					<Card.Content>
						<div class="max-h-52 space-y-2 overflow-auto text-xs text-muted-foreground">
							{#each unassignedComments as comment}
								<p class="rounded-sm border border-border bg-background/70 px-2 py-1">{comment}</p>
							{/each}
						</div>
					</Card.Content>
				</Card.Root>
			{/if}
		</div>
	{/if}

	{#if config}
		<div class="grid gap-4 xl:grid-cols-12">
			<Card.Root class="group relative flex max-h-200 flex-col overflow-hidden rounded-sm border-border bg-card shadow-sm transition-shadow duration-500 ease-[cubic-bezier(0.22,1,0.36,1)] xl:col-span-5">
				<div
					class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
				></div>
				<Card.Header class="border-b border-border pb-4">
					<div class="flex items-center gap-3">
						<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
							<FolderTreeIcon class="size-5" />
						</div>
						<div>
							<Card.Title>Objects & Fields</Card.Title>
							<Card.Description>Browse by parent object, then add or edit typed fields.</Card.Description>
						</div>
					</div>
				</Card.Header>
				<Card.Content class="min-h-0 space-y-4 overflow-auto">
					<div class="grid gap-2">
						<div class="flex items-center gap-2 text-xs font-medium text-muted-foreground">
							<SearchIcon class="size-3.5" />
							Search Fields
						</div>
						<Input bind:value={search} placeholder="Search field, comment, type, or value..." title="Filter parsed config settings." class="rounded-sm" />
					</div>

					<div class="grid gap-2">
						<div class="flex items-center justify-between gap-2">
							<span class="text-xs font-medium text-muted-foreground">Objects</span>
							<span class="rounded-xs border border-border bg-background px-2 py-0.5 text-[10px] text-muted-foreground">{groups.length} groups</span>
						</div>
						<div class="max-h-48 space-y-1 overflow-auto rounded-sm border border-border bg-background/60 p-1">
							{#each groups as group (group.id)}
								<button
									class={["grid w-full gap-1 rounded-xs px-2 py-2 text-left transition-colors hover:bg-muted/50", selectedGroupId === group.id ? "bg-muted text-foreground" : "text-muted-foreground"]}
									onclick={() => selectGroup(group.id)}
									title={`Show fields in ${group.label}`}
								>
									<span class="truncate font-mono text-xs">{group.label}</span>
									<span class="text-[10px] uppercase tracking-wide">{group.fieldCount} fields / {group.objectCount} objects</span>
									{#if group.comment}
										<span class="line-clamp-2 text-xs text-muted-foreground">{group.comment}</span>
									{/if}
								</button>
							{/each}
						</div>
					</div>

					<div class="grid gap-2 rounded-sm border border-border bg-background/60 p-3">
						<div class="flex items-center justify-between gap-2">
							<span class="text-xs font-medium text-muted-foreground">Add To {selectedGroup?.label ?? "Configuration"}</span>
						</div>
						<div class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_12rem]">
							<Input bind:value={newFieldName} placeholder="fieldName or 3" title="New field/object name. Use a number to append array-style entries." class="rounded-sm font-mono text-xs" />
							<Select.Root bind:value={newFieldType} type="single" items={fieldTypes.map((type) => ({ value: type.value, label: type.label }))}>
								<Select.Trigger title="Choose the type for the new config field." class="w-full rounded-sm font-mono text-xs">
									<span>{selectedFieldTypeLabel()}</span>
								</Select.Trigger>
								<Select.Content class="rounded-sm">
									{#each fieldTypes as type}
										<Select.Item value={type.value} label={type.label}>
											<div class="grid gap-0.5">
												<span class="font-mono text-xs">{type.label}</span>
												<span class="text-[10px] text-muted-foreground">{type.hint}</span>
											</div>
										</Select.Item>
									{/each}
								</Select.Content>
							</Select.Root>
						</div>
						<div class="grid gap-2 sm:grid-cols-2">
							<Button size="sm" onclick={addField} disabled={!newFieldName.trim()} title="Add a typed field to the selected object">
								<PlusIcon />
								Add Field
							</Button>
							<Button variant="outline" size="sm" onclick={addObject} disabled={!newFieldName.trim()} title="Create a nested object inside the selected object">
								<FolderTreeIcon />
								Add Object
							</Button>
						</div>
						<p class="text-[11px] leading-4 text-muted-foreground">Fields become editable values. Object and array types create nested categories that can contain more entries.</p>
					</div>

					<div class="max-h-104 overflow-auto rounded-sm border border-border bg-background/60">
						<div class="border-b border-border/70 bg-muted/30 px-3 py-2">
							<p class="font-mono text-xs text-foreground">{selectedGroup?.label ?? "Configuration"}</p>
							<p class="text-[10px] uppercase tracking-wide text-muted-foreground">{filteredSettings.length} matching fields</p>
						</div>
						{#each filteredSettings as setting (setting.id)}
							<button
								class={[
									"grid w-full gap-2 border-b border-border/70 px-3 py-3 text-left transition-colors last:border-0 hover:bg-muted/50",
									selected?.id === setting.id ? "bg-muted text-foreground" : "text-muted-foreground",
								]}
								onclick={() => (selectedId = setting.id)}
								title={`Edit ${displayLabel(setting)}`}
							>
								<div class="flex items-center justify-between gap-3">
									<span class="min-w-0 truncate font-mono text-xs">{displayLabel(setting)}</span>
									<span class={`shrink-0 rounded-xs border px-2 py-0.5 text-[10px] font-semibold uppercase ${typeClass(setting.type)}`}>{setting.type}</span>
								</div>
								{#if setting.comment}
									<span class="line-clamp-2 text-xs text-muted-foreground">{setting.comment}</span>
								{/if}
								<span class="truncate font-mono text-xs">{valueSummary(setting.value)}</span>
							</button>
						{/each}
						{#if filteredSettings.length === 0}
							<div class="grid gap-2 px-3 py-6 text-center text-xs text-muted-foreground">
								<span>No fields in this object yet.</span>
								<span>Add a field above to start building this section.</span>
							</div>
						{/if}
					</div>
				</Card.Content>
			</Card.Root>

			<Card.Root class="group relative flex max-h-200 flex-col overflow-hidden rounded-sm border-border bg-card shadow-sm transition-shadow duration-500 ease-[cubic-bezier(0.22,1,0.36,1)] xl:col-span-7">
				<div
					class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
				></div>
				<Card.Header class="border-b border-border pb-4">
					<div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
						<div>
							<Card.Title>{selected ? displayLabel(selected) : "No Setting Selected"}</Card.Title>
							<Card.Description>
								{selected?.comment ?? (selected?.editable ? "Edits are validated against the parsed Lua value type." : "This value is preserved in output but cannot be safely edited here.")}
							</Card.Description>
						</div>
						{#if selected}
							<Button variant="destructive" size="sm" onclick={removeSelected} title={`Remove ${displayLabel(selected)} from the generated config`}>
								<Trash2Icon />
								Remove
							</Button>
						{/if}
					</div>
				</Card.Header>
				<Card.Content class="min-h-0 space-y-4 overflow-auto">
					{#if selected}
						<div class="grid gap-3 md:grid-cols-4">
							<div class="rounded-sm border border-border bg-background/70 p-3">
								<p class="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">Parent</p>
								<p class="mt-1 truncate font-mono text-xs text-foreground">{parentLabel(selected)}</p>
							</div>
							<div class="rounded-sm border border-border bg-background/70 p-3">
								<p class="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">Field</p>
								<p class="mt-1 truncate font-mono text-xs text-foreground">{fieldName(selected)}</p>
							</div>
							<div class="rounded-sm border border-border bg-background/70 p-3">
								<p class="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">Type</p>
								<span class={`mt-1 inline-flex rounded-xs border px-2 py-0.5 text-[10px] font-semibold uppercase ${typeClass(selected.type)}`}>{selected.type}</span>
							</div>
							<div class="rounded-sm border border-border bg-background/70 p-3">
								<p class="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">State</p>
								<p class="mt-1 text-xs font-medium text-foreground">{baselineValues[selected.id] !== stringifyLuaValue(selected.value) ? "Changed" : "Original"}</p>
							</div>
						</div>

						<div class="grid gap-2 rounded-sm border border-border bg-background/60 p-3">
							<div class="flex items-center justify-between gap-2">
								<span class="text-xs font-medium text-muted-foreground">Value Editor</span>
								<span class="font-mono text-xs text-muted-foreground">{valueSummary(selected.value)}</span>
							</div>

							{#if selected.value.type === "string"}
								<label class="grid gap-2">
									<span class="text-xs font-medium text-muted-foreground">String Value</span>
									<Input value={selected.value.value} placeholder="Text value" title="String config value." oninput={updateString} class="rounded-sm font-mono" />
								</label>
							{:else if selected.value.type === "number"}
								<label class="grid gap-2">
									<span class="text-xs font-medium text-muted-foreground">Number Value</span>
									<Input type="number" value={selected.value.value} placeholder="0" title="Numeric config value." oninput={updateNumber} class="rounded-sm font-mono" />
								</label>
							{:else if selected.value.type === "boolean"}
								<div class="grid gap-2">
									<span class="text-xs font-medium text-muted-foreground">Boolean Value</span>
									<div class="grid grid-cols-2 gap-2">
										<Button
											variant="outline"
											class={booleanButtonClass(selected.value.value, "true")}
											aria-pressed={selected.value.value}
											onclick={() => updateBoolean(true)}
											title="Set value to true"
										>
											True
										</Button>
										<Button
											variant="outline"
											class={booleanButtonClass(!selected.value.value, "false")}
											aria-pressed={!selected.value.value}
											onclick={() => updateBoolean(false)}
											title="Set value to false"
										>
											False
										</Button>
									</div>
								</div>
							{:else if selected.value.type === "vector2" || selected.value.type === "vector3" || selected.value.type === "vector4"}
								<div class="grid gap-3">
									<span class="text-xs font-medium text-muted-foreground">Vector Components</span>
									<div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
										{#each selected.value.values as component, index}
											<label class="grid gap-2">
												<span class="text-xs font-medium text-muted-foreground">{["X", "Y", "Z", "W"][index]}</span>
												<Input
													type="number"
													value={component}
													placeholder="0.0"
													title="Vector component must be numeric."
													oninput={(event) => updateVector(index, event)}
													class="rounded-sm font-mono"
												/>
											</label>
										{/each}
									</div>
								</div>
							{:else if selected.value.type === "nil"}
								<p class="rounded-sm border border-border bg-background/70 px-3 py-2 text-sm text-muted-foreground">Nil values are preserved as `nil`.</p>
							{:else if selected.value.type === "raw"}
								<pre class="max-h-72 overflow-auto rounded-sm border border-red-400/20 bg-red-400/10 p-3 font-mono text-xs text-red-100">{selected.value.value}</pre>
							{:else}
								<p class="rounded-sm border border-border bg-background/70 px-3 py-2 text-sm text-muted-foreground">
									Empty {selected.value.type === "array" ? "array" : "object"} values are preserved in the generated output. Add entries through the selected object panel.
								</p>
							{/if}
						</div>

						<label class="grid gap-2">
							<span class="text-xs font-medium text-muted-foreground">Associated Comment</span>
							<textarea
								value={selected.comment ?? ""}
								placeholder="Add notes shown above this value in the exported config..."
								title="Comment lines that will be emitted directly above this config value."
								oninput={updateComment}
								class="min-h-24 rounded-sm border border-input bg-background px-3 py-2 text-sm text-foreground outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
							></textarea>
						</label>

						<div class="grid gap-2 rounded-sm border border-border bg-background/60 p-3">
							<div class="flex items-center justify-between gap-2">
								<span class="text-xs font-medium text-muted-foreground">Lua Preview</span>
								<span class="truncate font-mono text-[11px] text-muted-foreground">{selected.label}</span>
							</div>
							<pre class="max-h-28 overflow-auto rounded-xs border border-border bg-card/80 p-3 font-mono text-xs leading-5 whitespace-pre text-[#D4D4D4]">{#each selectedPreviewTokens as token}<span
										class={tokenClass(token.type)}>{token.text}</span
									>{/each}</pre>
						</div>
					{:else}
						<div class="grid min-h-80 place-items-center rounded-sm border border-dashed border-border bg-background/60 p-8 text-center">
							<div class="max-w-sm space-y-2">
								<p class="text-sm font-medium text-foreground">No editable setting selected</p>
								<p class="text-sm text-muted-foreground">Parse a config file, choose an object, then select a field to edit its typed value, comment, and generated Lua preview.</p>
							</div>
						</div>
					{/if}
				</Card.Content>
			</Card.Root>
		</div>

		<Card.Root class="group relative flex max-h-176 flex-col overflow-hidden rounded-sm border-border bg-card shadow-sm transition-shadow duration-500 ease-[cubic-bezier(0.22,1,0.36,1)]">
			<div
				class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
			></div>
			<Card.Header class="border-b border-border pb-4">
				<div class="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
					<div>
						<Card.Title>Generated Lua</Card.Title>
						<Card.Description>Normalized Lua output from the typed editor.</Card.Description>
					</div>
					<div class="flex flex-wrap gap-2">
						<Button variant="outline" onclick={syncGeneratedToSource} title="Move generated Lua back into the source editor and reparse it">
							<RotateCcwIcon />
							Use As Source
						</Button>
						<Button variant="outline" onclick={copyOutput} title="Copy generated Lua">
							<ClipboardIcon />
							Copy
						</Button>
						<Button onclick={downloadOutput} title="Download generated Lua">
							<DownloadIcon />
							Download
						</Button>
					</div>
				</div>
			</Card.Header>
			<Card.Content class="min-h-0 overflow-auto">
				<pre class="max-h-136 overflow-auto rounded-sm border border-border bg-background/70 p-4 font-mono text-xs leading-5 whitespace-pre text-[#D4D4D4]">{#each highlightedOutput as token}<span
							class={tokenClass(token.type)}>{token.text}</span
						>{/each}</pre>
			</Card.Content>
		</Card.Root>
	{/if}
</section>
