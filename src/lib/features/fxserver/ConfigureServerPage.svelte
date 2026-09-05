<script lang="ts">
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import ClipboardIcon from "@lucide/svelte/icons/clipboard";
	import EyeIcon from "@lucide/svelte/icons/eye";
	import EyeOffIcon from "@lucide/svelte/icons/eye-off";
	import FileTextIcon from "@lucide/svelte/icons/file-text";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import KeyRoundIcon from "@lucide/svelte/icons/key-round";
	import ListPlusIcon from "@lucide/svelte/icons/list-plus";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import SaveIcon from "@lucide/svelte/icons/save";
	import SearchIcon from "@lucide/svelte/icons/search";
	import Undo2Icon from "@lucide/svelte/icons/undo-2";
	import { onDestroy, onMount } from "svelte";
	import { confirm } from "@tauri-apps/plugin-dialog";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import PasswordInput from "$lib/components/ui/password-input.svelte";
	import * as Select from "$lib/components/ui/select/index.js";
	import { chooseFolder } from "$lib/core/selectFolder";
	import { databaseSession, formatMariaDBConnectionString, rememberDatabaseCredentials } from "$lib/core/databaseSession.svelte";
	import { listMariaDBDatabases, validateMariaDBCredentials, type MariaDBCredentials } from "$lib/modules/mariadb";
	import { readServerConfig, type ServerConfigFile, type ServerConfigResult } from "$lib/modules/fxserver";
	import { readConfigHistoryFile, saveConfigWithHistory } from "$lib/modules/configHistory";
	import ConfigHistoryPanel from "$lib/features/config-history/ConfigHistoryPanel.svelte";
	import ConfigDiff from "$lib/features/config-history/ConfigDiff.svelte";
	import { fxserverSettings, loadFxserverSettings, refreshTxDataProfiles, setServerProfile, setTxDataPath } from "./fxserverSettings.svelte";

	let dataPath = $state("");
	let profile = $state("");
	let result = $state<ServerConfigResult | null>(null);
	let selectedPath = $state("");
	let editorContent = $state("");
	let query = $state("");
	let busy = $state(false);
	let active = true;
	onDestroy(() => { active = false; });
	let saving = $state(false);
	let externalFile = $state<ServerConfigFile | null>(null);
	let externalReviewed = $state(false);
	let checkingExternal = false;
	let notice = $state("");
	let noticeLevel = $state<"success" | "error">("success");
	let drafts = $state<Record<string, string>>({});
	let editorElement = $state<HTMLTextAreaElement | null>(null);
	let selectedFxDatabase = $state(databaseSession.credentials?.database ?? "fxserver");
	let dbCredentials = $state<MariaDBCredentials>({
		host: databaseSession.credentials?.host ?? databaseSession.defaults.host,
		port: databaseSession.credentials?.port ?? databaseSession.defaults.port,
		username: databaseSession.credentials?.username ?? databaseSession.defaults.username,
		password: databaseSession.credentials?.password ?? "",
		database: databaseSession.credentials?.database ?? "fxserver",
	});
	let fxDatabases = $state<string[]>([]);
	let showDbConnectionString = $state(false);
	let dbCredentialsReady = $state(Boolean(databaseSession.credentials));
	let dbNotice = $state("");
	let dbNoticeLevel = $state<"success" | "error">("success");
	let serverPrincipalIdentifier = $state("");
	let serverPrincipalGroup = $state("group.admin");
	let serverPrincipalComment = $state("");
	let acePrincipal = $state("group.admin");
	let aceObject = $state("command");
	let aceAccess = $state("allow");
	let inheritanceChild = $state("group.admin");
	let inheritanceParent = $state("group.mod");
	let resourceName = $state("");
	let resourceAceObject = $state("command");
	let resourceAceAccess = $state("allow");

	const profileOptions = $derived(fxserverSettings.profiles.map((profileName) => ({ value: profileName, label: profileName })));
	const filteredFiles = $derived(
		(result?.files ?? []).filter((file) => {
			const haystack = `${file.name} ${file.path} ${file.content}`.toLowerCase();
			return !query.trim() || haystack.includes(query.trim().toLowerCase());
		}),
	);
	const selectedFile = $derived((result?.files ?? []).find((file) => file.path === selectedPath) ?? null);
	const historyRequest = $derived(result && selectedFile ? { txDataPath: result.txDataPath, profile: result.profile, path: selectedFile.path } : null);
	$effect(() => { editorContent; externalFile; externalReviewed = false; });
	const selectedFileName = $derived(selectedFile?.name.toLowerCase() ?? "");
	const serverCfgSelected = $derived(selectedFileName === "server.cfg");
	const permissionsCfgSelected = $derived(selectedFileName === "permissions.cfg");
	const dirty = $derived(Boolean(selectedFile && editorContent !== selectedFile.content));
	const richEditor = $derived(editorContent.length <= 200_000 && editorContent.split("\n", 2001).length <= 2000);
	const lineNumbers = $derived(richEditor ? Array.from({ length: Math.max(editorContent.split("\n").length, 1) }, (_, index) => index + 1) : []);
	const editorContentHeight = $derived(`${Math.max(640, lineNumbers.length * 20 + 24)}px`);
	const rconLineInSelectedFile = $derived(selectedFile && result?.rconPasswordFile === selectedFile.name ? (result.rconPasswordLine ?? null) : null);
	const rconlogLineInSelectedFile = $derived(selectedFile?.name.toLowerCase() === "server.cfg" ? (result?.rconlogLine ?? null) : null);
	const stats = $derived({
		files: result?.files.length ?? 0,
		lines: editorContent ? editorContent.split("\n").length : 0,
		chars: editorContent.length,
	});
	const highlightedLines = $derived(richEditor ? editorContent.split("\n").map((line) => highlightCfgLine(line)) : []);
	const rconReady = $derived(Boolean(result?.rconPasswordFound && result?.rconlogFound));
	const dbConnectionString = $derived(dbCredentialsReady ? formatMariaDBConnectionString({ ...dbCredentials, database: selectedFxDatabase }) : databaseSession.connectionString);
	const fxDatabaseOptions = $derived(fxDatabases.map((database) => ({ value: database, label: database })));
	const popularCfgValues = $derived({
		hostname: getCfgValue(editorContent, "sv_hostname"),
		maxClients: getCfgValue(editorContent, "sv_maxclients"),
		projectName: getCfgValue(editorContent, "sets sv_projectName"),
		projectDescription: getCfgValue(editorContent, "sets sv_projectDesc"),
		licenseKey: getCfgValue(editorContent, "sv_licenseKey"),
		mysqlConnectionString: getCfgValue(editorContent, "set mysql_connection_string") || getCfgValue(editorContent, "setr mysql_connection_string"),
	});
	const rconCommands = [
		{ command: "say <message>", description: "Send a chat message to all players." },
		{ command: "start <resource-name>", description: "Start a server resource." },
		{ command: "stop <resource-name>", description: "Stop a server resource." },
		{ command: "restart <resource-name>", description: "Restart a server resource." },
		{ command: "ensure <resource-name>", description: "Start a resource if it is not already running." },
		{ command: "refresh", description: "Reload resources from the resource directory." },
		{ command: "clear", description: "Clear server console output." },
		{ command: "quit", description: "Shut down the server cleanly." },
	];
	const editorLineStyle = "height: 20px; line-height: 20px;";

	onMount(() => {
		loadFxserverSettings();
		dataPath = fxserverSettings.txDataPath;
		profile = fxserverSettings.profile;
		void (async () => {
			await refreshTxDataProfiles();
			if (!active) return;
			dataPath = fxserverSettings.txDataPath;
			profile = fxserverSettings.profile;
			if (databaseSession.credentials) {
				await validateFxDatabaseCredentials(false);
			}
			if (!active) return;
			if (dataPath.trim() && profile.trim()) {
				await loadConfig();
			}
		})();
	});

	async function loadConfig() {
		if (!active || busy || saving) return;
		busy = true;
		const discard = await confirmDiscardDrafts();
		if (!active) return;
		if (!discard) {
			dataPath = result?.txDataPath ?? dataPath;
			profile = result?.profile ?? profile;
			setTxDataPath(dataPath);
			setServerProfile(profile);
			busy = false;
			return;
		}
		notice = "";
		setTxDataPath(dataPath);
		setServerProfile(profile);

		try {
			const nextResult = await readServerConfig({
				txDataPath: dataPath.trim(),
				profile: profile.trim(),
			});
			result = nextResult;
			drafts = {};
			selectFile(nextResult.files.find((file) => file.name === "server.cfg")?.path ?? nextResult.files[0]?.path ?? "", false);
			notice = `Loaded ${nextResult.files.length} config file${nextResult.files.length === 1 ? "" : "s"}.`;
			noticeLevel = "success";
		} catch (error) {
			result = null;
			selectedPath = "";
			editorContent = "";
			notice = error instanceof Error ? error.message : String(error);
			noticeLevel = "error";
		} finally {
			busy = false;
		}
	}

	async function validateFxDatabaseCredentials(showNotice = true) {
		if (!active) return;
		const original = { ...dbCredentials };
		const revision = databaseSession.revision;
		dbCredentialsReady = false;
		if (showNotice) dbNotice = "";
		fxDatabases = [];

		try {
			await validateMariaDBCredentials(original);
			const databases = await listMariaDBDatabases(original);
			if (!active || JSON.stringify(original) !== JSON.stringify(dbCredentials) || !rememberDatabaseCredentials(original, revision)) return;
			fxDatabases = databases;
			dbCredentialsReady = true;
			dbCredentials.database = selectedFxDatabase;
			if (!selectedFxDatabase && fxDatabases.length) {
				selectedFxDatabase = fxDatabases[0];
				dbCredentials.database = selectedFxDatabase;
			}
			if (showNotice) {
				dbNotice = "Database credentials validated.";
				dbNoticeLevel = "success";
			}
		} catch (error) {
			if (showNotice) {
				dbNotice = error instanceof Error ? error.message : String(error);
				dbNoticeLevel = "error";
			}
		}
	}

	async function copyDbConnectionString() {
		if (!dbConnectionString) return;
		await navigator.clipboard.writeText(dbConnectionString);
		dbNotice = "Connection string copied.";
		dbNoticeLevel = "success";
	}

	async function chooseTxDataFolder() {
		notice = "";
		const selectedFolder = await chooseFolder();
		if (!selectedFolder || !active) return;
		if (!await confirmDiscardDrafts() || !active) return;

		dataPath = selectedFolder;
		profile = "";
		clearLoadedConfig();
		setTxDataPath(selectedFolder);
		setServerProfile("");
		await refreshTxDataProfiles();
	}

	async function handleTxDataChange(event: Event) {
		const nextPath = (event.currentTarget as HTMLInputElement).value;
		if (!await confirmDiscardDrafts()) { dataPath = result?.txDataPath ?? dataPath; return; }
		if (!active) return;
		dataPath = nextPath;
		profile = "";
		clearLoadedConfig();
		setTxDataPath(dataPath);
		setServerProfile("");
		await refreshTxDataProfiles();
	}

	function handleProfileChange(nextProfile: string) {
		profile = nextProfile;
		setServerProfile(nextProfile);
		if (dataPath.trim() && nextProfile.trim()) {
			void loadConfig();
		}
	}

	async function confirmDiscardDrafts() {
		const hasDrafts = dirty || Object.entries(drafts).some(([path, content]) => result?.files.find((file) => file.path === path)?.content !== content);
		return !hasDrafts || await confirm("Discard unsaved config drafts and reload this profile?", { title: "Reload configuration", kind: "warning" });
	}

	function clearLoadedConfig() {
		result = null;
		drafts = {};
		selectedPath = "";
		editorContent = "";
		externalFile = null;
	}

	function selectFile(path: string, keepCurrentDraft = true) {
		if (saving) return;
		externalFile = null;
		externalReviewed = false;
		if (!path) {
			selectedPath = "";
			editorContent = "";
			return;
		}

		if (keepCurrentDraft && selectedPath) {
			drafts = { ...drafts, [selectedPath]: editorContent };
		}

		const file = result?.files.find((item) => item.path === path);
		selectedPath = path;
		editorContent = drafts[path] ?? file?.content ?? "";
	}

	function revertFile() {
		if (!selectedFile) return;
		const { [selectedFile.path]: _removed, ...remainingDrafts } = drafts;
		drafts = remainingDrafts;
		editorContent = selectedFile.content;
		notice = `${selectedFile.name} reverted.`;
		noticeLevel = "success";
	}

	async function saveFile() {
		if (!selectedFile || !historyRequest || saving || busy || externalFile) return;
		const target = historyRequest;
		const expected = selectedFile.content;
		const submitted = editorContent;

		saving = true;
		notice = "";

		try {
			const savedFile = await saveConfigWithHistory(target, expected, submitted);
			acceptFile(savedFile, editorContent === submitted);
			notice = `${savedFile.name} saved with encrypted history.`;
			noticeLevel = "success";
		} catch (error) {
			notice = error instanceof Error ? error.message : String(error);
			noticeLevel = "error";
			if (notice.includes("CONFIG_CHANGED") || notice.includes("config was saved")) await checkExternal(true);
		} finally {
			saving = false;
		}
	}

	function acceptFile(savedFile: ServerConfigFile, replaceDraft = true) {
			const nextFiles = result?.files.map((file) => (file.path === savedFile.path ? savedFile : file)) ?? [];
			const rcon = findRconPassword(nextFiles);
			const rconlog = findRconlog(nextFiles);
			result = result
				? {
						...result,
						files: nextFiles,
						rconPasswordFound: Boolean(rcon),
						rconPasswordFile: rcon?.file.name ?? null,
						rconPasswordLine: rcon?.line ?? null,
						rconlogFound: Boolean(rconlog),
						rconlogLine: rconlog?.line ?? null,
					}
				: result;
			if (replaceDraft) {
				drafts = { ...drafts, [savedFile.path]: savedFile.content };
				if (selectedPath === savedFile.path) editorContent = savedFile.content;
			}
			externalFile = null;
			externalReviewed = false;
	}

	async function checkExternal(force = false) {
		if (!historyRequest || !selectedFile || checkingExternal || busy || (saving && !force)) return;
		const target = historyRequest;
		checkingExternal = true;
		try {
			const current = await readConfigHistoryFile(target);
			if (selectedPath === target.path && current.content !== selectedFile?.content) {
				externalFile = current;
				externalReviewed = false;
			}
		} catch (error) {
			if (selectedPath === target.path) { notice = String(error); noticeLevel = "error"; }
		} finally { checkingExternal = false; }
	}

	function reloadExternal(keepDraft = false) {
		if (!externalFile || (keepDraft && !externalReviewed)) return;
		acceptFile(externalFile, !keepDraft);
		if (keepDraft) {
			notice = "The reviewed disk version is now the baseline. Review the draft before saving.";
			noticeLevel = "success";
		}
	}

	function findRconPassword(files: ServerConfigFile[]) {
		for (const file of files.filter((item) => item.name.toLowerCase() === "server.cfg")) {
			const index = file.content.split("\n").findIndex((line) => isRconPasswordLine(line));
			if (index >= 0) return { file, line: index + 1 };
		}

		return null;
	}

	function findRconlog(files: ServerConfigFile[]) {
		for (const file of files.filter((item) => item.name.toLowerCase() === "server.cfg")) {
			const index = file.content.split("\n").findIndex((line) => isRconlogLine(line));
			if (index >= 0) return { file, line: index + 1 };
		}

		return null;
	}

	function isRconPasswordLine(line: string) {
		const trimmed = line.trimStart();
		if (trimmed.startsWith("#") || trimmed.startsWith("//")) return false;
		const lower = trimmed.toLowerCase();
		return lower.startsWith("rcon_password") || lower.startsWith("set rcon_password");
	}

	function isRconlogLine(line: string) {
		const trimmed = line.trimStart();
		if (trimmed.startsWith("#") || trimmed.startsWith("//")) return false;
		const [command, resource, extra] = trimmed.split(/\s+/);
		return command?.toLowerCase() === "ensure" && resource?.toLowerCase() === "rconlog" && !extra;
	}

	function selectServerCfg() {
		const serverCfg = result?.files.find((file) => file.name.toLowerCase() === "server.cfg");
		if (serverCfg) selectFile(serverCfg.path);
		return serverCfg;
	}

	function selectPermissionsCfg() {
		const permissionsCfg = result?.files.find((file) => file.name.toLowerCase() === "permissions.cfg");
		if (permissionsCfg) selectFile(permissionsCfg.path);
		return permissionsCfg;
	}

	function autofillRconConfig() {
		const serverCfg = selectServerCfg();
		if (!serverCfg) {
			notice = "server.cfg was not found in the resolved server data path.";
			noticeLevel = "error";
			return;
		}

		const currentContent = drafts[serverCfg.path] ?? serverCfg.content;
		const lines = currentContent.split("\n");
		const additions = [];
		if (!lines.some(isRconlogLine)) additions.push("ensure rconlog");
		if (!lines.some(isRconPasswordLine)) additions.push('set rcon_password "your-secure-password"');

		if (!additions.length) {
			notice = "server.cfg already has the RCON setup lines.";
			noticeLevel = "success";
			return;
		}

		const base = currentContent.trimEnd();
		editorContent = `${base}${base ? "\n\n" : ""}# RCON setup\n${additions.join("\n")}\n`;
		drafts = { ...drafts, [serverCfg.path]: editorContent };
		notice = "Added missing RCON setup lines to server.cfg. Replace the password before saving.";
		noticeLevel = "success";
	}

	function autofillCommand(command: string) {
		const serverCfg = selectServerCfg();
		if (!serverCfg) {
			notice = "server.cfg was not found in the resolved server data path.";
			noticeLevel = "error";
			return;
		}

		const base = (drafts[serverCfg.path] ?? serverCfg.content).trimEnd();
		editorContent = `${base}${base ? "\n\n" : ""}${command}\n`;
		drafts = { ...drafts, [serverCfg.path]: editorContent };
		notice = `${command} added to server.cfg.`;
		noticeLevel = "success";
	}

	function appendToConfigFile(file: ServerConfigFile | undefined, snippet: string, label: string) {
		if (!file) {
			notice = `${label} config file was not found in the resolved server data path.`;
			noticeLevel = "error";
			return;
		}

		selectFile(file.path);
		const base = (drafts[file.path] ?? file.content).trimEnd();
		editorContent = `${base}${base ? "\n\n" : ""}${snippet.trimEnd()}\n`;
		drafts = { ...drafts, [file.path]: editorContent };
		notice = `${label} helper added to ${file.name}.`;
		noticeLevel = "success";
	}

	function cleanCfgValue(value: string) {
		return value.trim().replace(/[\r\n]+/g, " ");
	}

	function addServerPrincipal() {
		const identifier = cleanCfgValue(serverPrincipalIdentifier);
		const group = cleanCfgValue(serverPrincipalGroup);
		const comment = cleanCfgValue(serverPrincipalComment);
		if (!identifier || !group) {
			notice = "Enter both an identifier and a group before adding a principal.";
			noticeLevel = "error";
			return;
		}

		appendToConfigFile(selectServerCfg(), `## Permissions ##\nadd_principal ${identifier} ${group}${comment ? ` #${comment.replace(/^#\s*/, "")}` : ""}`, "Server permissions");
	}

	function addPermissionsAce() {
		const principal = cleanCfgValue(acePrincipal);
		const object = cleanCfgValue(aceObject);
		const access = cleanCfgValue(aceAccess);
		if (!principal || !object || !access) {
			notice = "Enter a principal, permission, and access value before adding an ACE rule.";
			noticeLevel = "error";
			return;
		}

		appendToConfigFile(selectPermissionsCfg(), `add_ace ${principal} ${object} ${access}`, "permissions.cfg");
	}

	function addPermissionsInheritance() {
		const child = cleanCfgValue(inheritanceChild);
		const parent = cleanCfgValue(inheritanceParent);
		if (!child || !parent) {
			notice = "Enter both groups before adding inheritance.";
			noticeLevel = "error";
			return;
		}

		appendToConfigFile(selectPermissionsCfg(), `add_principal ${child} ${parent}`, "permissions.cfg");
	}

	function addResourceAce() {
		const resource = cleanCfgValue(resourceName).replace(/^resource\./, "");
		const object = cleanCfgValue(resourceAceObject);
		const access = cleanCfgValue(resourceAceAccess);
		if (!resource || !object || !access) {
			notice = "Enter a resource, permission, and access value before adding a resource ACE.";
			noticeLevel = "error";
			return;
		}

		appendToConfigFile(selectPermissionsCfg(), `add_ace resource.${resource} ${object} ${access}`, "permissions.cfg");
	}

	function getCfgValue(content: string, command: string) {
		const lowerCommand = command.toLowerCase();
		const line = content.split("\n").find((item) => {
			const trimmed = item.trimStart();
			return !trimmed.startsWith("#") && !trimmed.startsWith("//") && trimmed.toLowerCase().startsWith(lowerCommand);
		});
		if (!line) return "";
		const value = line.trimStart().slice(command.length).trim();
		return value.replace(/^"(.*)"$/, "$1");
	}

	function setPopularCfgValue(command: string, value: string, quote = true) {
		const serverCfg = selectServerCfg();
		if (!serverCfg) {
			notice = "server.cfg was not found in the resolved server data path.";
			noticeLevel = "error";
			return;
		}

		const lineValue = quote ? `"${value.replace(/"/g, '\\"')}"` : value;
		const nextLine = `${command} ${lineValue}`.trimEnd();
		const lowerCommand = command.toLowerCase();
		const lines = editorContent.split("\n");
		const index = lines.findIndex((line) => {
			const trimmed = line.trimStart();
			return !trimmed.startsWith("#") && !trimmed.startsWith("//") && trimmed.toLowerCase().startsWith(lowerCommand);
		});

		if (index >= 0) {
			lines[index] = nextLine;
		} else {
			if (lines.length && lines.at(-1)?.trim()) lines.push("");
			lines.push(nextLine);
		}

		editorContent = lines.join("\n");
		drafts = { ...drafts, [serverCfg.path]: editorContent };
	}

	function updatePopularValue(command: string, event: Event, quote = true) {
		setPopularCfgValue(command, (event.currentTarget as HTMLInputElement).value, quote);
	}

	function setDbConnectionStringInCfg() {
		if (!dbConnectionString) {
			notice = "Validate database credentials before setting the connection string.";
			noticeLevel = "error";
			return;
		}

		setPopularCfgValue("set mysql_connection_string", dbConnectionString);
		notice = "MySQL connection string set in server.cfg.";
		noticeLevel = "success";
	}

	function formatModified(file: ServerConfigFile) {
		if (!file.modified) return "Unknown";
		return new Date(file.modified * 1000).toLocaleString(undefined, {
			month: "short",
			day: "2-digit",
			hour: "2-digit",
			minute: "2-digit",
		});
	}

	function handleEditorKeydown(event: KeyboardEvent) {
		if (!(event.ctrlKey || event.metaKey)) return;

		const key = event.key.toLowerCase();
		if (key === "s") {
			event.preventDefault();
			if (selectedFile && dirty && !saving) void saveFile();
		}

		if (key === "z") {
			event.preventDefault();
			if (dirty) revertFile();
		}
	}

	function escapeHtml(value: string) {
		return value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
	}

	function highlightCfgLine(line: string) {
		const escapedLine = escapeHtml(line);
		const trimmed = line.trimStart();
		const leading = line.slice(0, line.length - trimmed.length);

		if (!line) return "&nbsp;";
		if (trimmed.startsWith("#") || trimmed.startsWith("//")) {
			return `<span class="text-emerald-300/70">${escapedLine || "&nbsp;"}</span>`;
		}

		const commandMatch = trimmed.match(/^([A-Za-z_][\w.-]*)(\s+)?(.*)$/);
		if (!commandMatch) return escapedLine;

		const [, command, spacing = "", rest = ""] = commandMatch;
		const lowerCommand = command.toLowerCase();
		const commandClass =
			lowerCommand === "ensure" || lowerCommand === "start" || lowerCommand === "stop" || lowerCommand === "restart"
				? "text-sky-300"
				: lowerCommand === "set" || lowerCommand === "setr" || lowerCommand === "sets"
					? "text-violet-300"
					: lowerCommand.includes("rcon")
						? "text-amber-200"
						: "text-cyan-200";
		const highlightedRest = highlightCfgValue(rest);

		return `${escapeHtml(leading)}<span class="${commandClass}">${escapeHtml(command)}</span>${escapeHtml(spacing)}${highlightedRest}`;
	}

	function highlightCfgValue(value: string) {
		const matcher = /"[^"]*"|\btrue\b|\bfalse\b|\bnull\b|\b\d+(\.\d+)?\b/gi;
		let cursor = 0;
		let output = "";

		for (const match of value.matchAll(matcher)) {
			const token = match[0];
			const index = match.index ?? 0;
			output += escapeHtml(value.slice(cursor, index));
			output += `<span class="${cfgValueClass(token)}">${escapeHtml(token)}</span>`;
			cursor = index + token.length;
		}

		return output + escapeHtml(value.slice(cursor));
	}

	function cfgValueClass(token: string) {
		if (token.startsWith('"')) return "text-amber-200";
		if (/^(true|false|null)$/i.test(token)) return "text-fuchsia-200";
		return "text-orange-200";
	}
</script>

<svelte:window onfocus={() => void checkExternal()} />

<section class="space-y-6">
	<div class="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">FXServer</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Configure Server</h1>
			<p class="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">Edit the `.cfg` files from the server data path resolved through your selected txData profile.</p>
		</div>
		<Button variant="outline" onclick={loadConfig} disabled={busy || saving || !dataPath.trim() || !profile.trim()} title="Reload config files from disk">
			<RefreshCwIcon class={busy ? "animate-spin" : undefined} />
			Reload
		</Button>
	</div>

	<Card.Root class="overflow-hidden rounded-sm border-border bg-card shadow-sm">
		<Card.Header class="border-b border-border pb-4">
			<div class="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between">
				<div class="min-w-0">
					<Card.Title>Profile Source</Card.Title>
					<Card.Description class="mt-1 truncate font-mono text-xs">
						{result?.profileConfigPath ?? (dataPath.trim() && profile.trim() ? `${dataPath}\\${profile}\\config.json` : "Choose txData and profile")}
					</Card.Description>
				</div>
				<div class="grid gap-2 sm:grid-cols-[minmax(0,22rem)_auto]">
					<Input bind:value={dataPath} onchange={handleTxDataChange} disabled={busy || saving} placeholder="C:\FiveM\txData" title="Folder containing txAdmin profile folders." class="rounded-sm font-mono text-xs" />
					<Button variant="outline" onclick={chooseTxDataFolder} disabled={busy || saving} title="Browse for the txData folder">
						<FolderOpenIcon />
						Browse
					</Button>
				</div>
			</div>
		</Card.Header>
		<Card.Content class="space-y-4">
			<div class="grid gap-3 lg:grid-cols-[minmax(0,0.55fr)_minmax(0,1fr)_auto] lg:items-end">
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Profile</span>
					<Select.Root bind:value={profile} type="single" items={profileOptions} onValueChange={handleProfileChange} disabled={busy || saving}>
						<Select.Trigger title="Choose the txData profile folder" class="w-full rounded-sm font-mono text-xs">
							{profile || "Choose profile"}
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
				</label>
				<div class="rounded-sm border border-border bg-background/60 px-3 py-2">
					<p class="text-xs text-muted-foreground">Resolved Server Data Path</p>
					<p class="mt-1 truncate font-mono text-xs text-foreground">{result?.dataPath ?? "Load a profile to resolve dataPath from config.json."}</p>
				</div>
				<div class="rounded-sm border border-border bg-background/60 px-3 py-2 text-right">
					<p class="text-xs text-muted-foreground">Config Files</p>
					<p class="mt-1 font-mono text-sm font-semibold text-foreground">{stats.files}</p>
				</div>
			</div>

			{#if fxserverSettings.profileError}
				<Notice tone="error" message={fxserverSettings.profileError} onDismiss={() => (fxserverSettings.profileError = "")} />
			{/if}

			{#if notice}
				<Notice tone={noticeLevel} message={notice} onDismiss={() => (notice = "")} />
			{/if}
		</Card.Content>
	</Card.Root>

	<Card.Root class="overflow-hidden rounded-sm border-border bg-card shadow-sm">
		<Card.Header class="border-b border-border pb-4">
			<div class="flex items-center gap-3">
				<div class="flex size-9 shrink-0 items-center justify-center rounded-sm border border-sky-400/30 bg-sky-400/10 text-sky-200">
					<KeyRoundIcon class="size-4" />
				</div>
				<div>
					<Card.Title>Database Connection String</Card.Title>
					<Card.Description>Validate the database user you want FXServer resources to use, then copy the generated connection string.</Card.Description>
				</div>
			</div>
		</Card.Header>
		<Card.Content class="space-y-4">
			<div class="grid gap-3 md:grid-cols-5">
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Host</span>
					<Input bind:value={dbCredentials.host} placeholder="localhost" class="rounded-sm font-mono text-xs" />
				</label>
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Port</span>
					<Input type="number" bind:value={dbCredentials.port} placeholder="3306" class="rounded-sm font-mono text-xs" />
				</label>
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">User</span>
					<Input bind:value={dbCredentials.username} placeholder="fxserver" class="rounded-sm font-mono text-xs" />
				</label>
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Password</span>
					<PasswordInput bind:value={dbCredentials.password} placeholder="User password" class="rounded-sm font-mono text-xs" />
				</label>
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Database</span>
					<Select.Root bind:value={selectedFxDatabase} type="single" items={fxDatabaseOptions} disabled={!dbCredentialsReady || !fxDatabaseOptions.length}>
						<Select.Trigger title="Choose database for the connection string" class="w-full rounded-sm font-mono text-xs">
							{selectedFxDatabase || "Choose database"}
						</Select.Trigger>
						<Select.Content class="rounded-sm">
							{#if fxDatabaseOptions.length}
								{#each fxDatabaseOptions as option}
									<Select.Item value={option.value} label={option.label}>
										{option.label}
									</Select.Item>
								{/each}
							{:else}
								<Select.Item value="" label="Validate first" disabled>Validate first</Select.Item>
							{/if}
						</Select.Content>
					</Select.Root>
				</label>
			</div>
			<div class="grid gap-2 lg:grid-cols-[minmax(0,1fr)_auto_auto_auto]">
				{#if showDbConnectionString}
					<code class="truncate rounded-sm border border-border bg-background/70 px-3 py-2 font-mono text-xs text-foreground">
						{dbConnectionString || "Validate credentials to generate a connection string."}
					</code>
				{:else}
					<div class="rounded-sm border border-border bg-background/70 px-3 py-2 text-xs text-muted-foreground">Connection string hidden.</div>
				{/if}
				<Button variant="outline" onclick={() => validateFxDatabaseCredentials()} disabled={busy} title="Validate these database credentials">
					<KeyRoundIcon />
					Validate
				</Button>
				<Button
					variant="outline"
					onclick={() => (showDbConnectionString = !showDbConnectionString)}
					disabled={!dbConnectionString}
					title={showDbConnectionString ? "Hide connection string" : "Show connection string"}
				>
					{#if showDbConnectionString}
						<EyeOffIcon />
						Hide
					{:else}
						<EyeIcon />
						Show
					{/if}
				</Button>
				<Button variant="outline" onclick={copyDbConnectionString} disabled={!dbConnectionString} title="Copy database connection string">
					<ClipboardIcon />
					Copy
				</Button>
			</div>
			{#if dbNotice}
				<Notice tone={dbNoticeLevel} message={dbNotice} onDismiss={() => (dbNotice = "")} />
			{/if}
		</Card.Content>
	</Card.Root>

	{#if result}
		{#if rconReady}
			<div class="rounded-sm border border-emerald-400/30 bg-emerald-400/10 px-4 py-3 text-sm text-emerald-100">
				<div class="flex items-start gap-2">
					<KeyRoundIcon class="mt-0.5 size-4 shrink-0" />
					<p>
						RCON is configured in `server.cfg`: `ensure rconlog`{#if result.rconlogLine}
							on line {result.rconlogLine}{/if} and `rcon_password`{#if result.rconPasswordLine}
							on line
							{result.rconPasswordLine}{/if}. Use that password in Manage Server's Console card.
					</p>
				</div>
			</div>
		{:else}
			<div class="rounded-sm border border-red-400/30 bg-red-400/10 px-4 py-3 text-sm text-red-100">
				<div class="flex items-start gap-2">
					<AlertCircleIcon class="mt-0.5 size-4 shrink-0" />
					<p>
						`server.cfg` is missing {[!result.rconlogFound ? "`ensure rconlog`" : "", !result.rconPasswordFound ? '`set rcon_password "your-secure-password"`' : ""].filter(Boolean).join(" and ")}. Add
						the missing line{!result.rconlogFound && !result.rconPasswordFound ? "s" : ""} before using command input in Manage Server's Console card.
					</p>
				</div>
			</div>
		{/if}

		<div class="grid gap-4 xl:grid-cols-[20rem_minmax(0,1fr)]">
			<Card.Root class="overflow-hidden rounded-sm border-border bg-card shadow-sm">
				<Card.Header class="border-b border-border pb-4">
					<Card.Title>Config Files</Card.Title>
					<Card.Description>{filteredFiles.length} visible of {result.files.length} `.cfg` files.</Card.Description>
				</Card.Header>
				<Card.Content class="space-y-3">
					<div class="relative">
						<SearchIcon class="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground" />
						<Input bind:value={query} placeholder="Search files and content..." title="Filter config files." class="rounded-sm pl-9" />
					</div>
					<div class="max-h-152 overflow-auto rounded-sm border border-border bg-background/50">
						{#if filteredFiles.length}
							{#each filteredFiles as file (file.path)}
								<button
									type="button"
									class={`flex w-full items-start gap-3 border-b border-border/70 px-3 py-3 text-left transition-colors last:border-b-0 hover:bg-accent ${selectedPath === file.path ? "bg-accent text-accent-foreground" : ""}`}
									onclick={() => selectFile(file.path)}
									disabled={saving || busy}
									title={file.path}
								>
									<FileTextIcon class={`mt-0.5 size-4 shrink-0 ${file.hasRconPassword || file.hasRconlog ? "text-amber-200" : "text-muted-foreground"}`} />
									<span class="min-w-0 flex-1">
										<span class="flex items-center gap-2">
											<span class="truncate text-sm font-medium">{file.name}</span>
											{#if drafts[file.path] !== undefined && drafts[file.path] !== file.content}
												<span class="rounded-xs border border-primary/30 bg-primary/10 px-1.5 py-0.5 text-[10px] text-primary">edited</span>
											{/if}
										</span>
										<span class="mt-1 block truncate font-mono text-[11px] text-muted-foreground">{formatModified(file)}</span>
									</span>
								</button>
							{/each}
						{:else}
							<div class="px-3 py-8 text-center text-sm text-muted-foreground">No config files match the current search.</div>
						{/if}
					</div>
				</Card.Content>
			</Card.Root>

			<Card.Root class="overflow-hidden rounded-sm border-border bg-card shadow-sm">
				<Card.Header class="border-b border-border pb-4">
					<div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
						<div class="min-w-0">
							<Card.Title>{selectedFile?.name ?? "No File Selected"}</Card.Title>
							<Card.Description class="mt-1 truncate font-mono text-xs">{selectedFile?.path ?? "Choose a config file to edit."}</Card.Description>
						</div>
						<div class="flex flex-wrap gap-2">
							<Button variant="outline" onclick={revertFile} disabled={!dirty} title="Discard unsaved changes in this file">
								<Undo2Icon />
								Revert
							</Button>
							<Button onclick={saveFile} disabled={!selectedFile || !dirty || saving || busy || Boolean(externalFile)} title="Save this config file">
								<SaveIcon />
								{saving ? "Saving" : "Save"}
							</Button>
						</div>
					</div>
				</Card.Header>
				<Card.Content class="space-y-3">
					<div class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
						<span class="rounded-sm border border-border bg-background/70 px-2 py-1">{stats.lines} lines</span>
						<span class="rounded-sm border border-border bg-background/70 px-2 py-1">{stats.chars} characters</span>
						{#if dirty}
							<span class="rounded-sm border border-primary/30 bg-primary/10 px-2 py-1 text-primary">Unsaved changes</span>
						{/if}
						{#if rconLineInSelectedFile}
							<span class="rounded-sm border border-amber-400/30 bg-amber-400/10 px-2 py-1 text-amber-100">rcon_password line {rconLineInSelectedFile}</span>
						{/if}
						{#if rconlogLineInSelectedFile}
							<span class="rounded-sm border border-emerald-400/30 bg-emerald-400/10 px-2 py-1 text-emerald-100">rconlog line {rconlogLineInSelectedFile}</span>
						{/if}
					</div>
					{#if !richEditor}
						<textarea bind:this={editorElement} bind:value={editorContent} disabled={!selectedFile || saving || busy} spellcheck="false" wrap="off" onkeydown={handleEditorKeydown} title="Server config editor" class="h-160 w-full resize-none overflow-auto rounded-sm border border-border bg-background/70 p-3 font-mono text-xs leading-5"></textarea>
					{:else}
					<div class="h-160 overflow-auto rounded-sm border border-border bg-background/70 font-mono text-xs">
						<div class="grid min-h-full grid-cols-[3.5rem_minmax(max-content,1fr)]">
							<div class="border-r border-border bg-muted/30 py-3 text-right text-muted-foreground select-none" style={`height: ${editorContentHeight};`}>
								{#each lineNumbers as line}
									<div
										style={editorLineStyle}
										class={`px-2 ${line === rconLineInSelectedFile ? "bg-amber-400/20 text-amber-100" : line === rconlogLineInSelectedFile ? "bg-emerald-400/20 text-emerald-100" : ""}`}
									>
										{line}
									</div>
								{/each}
							</div>
							<div class="relative min-w-max overflow-hidden" style={`height: ${editorContentHeight};`}>
								<div class="pointer-events-none min-w-max px-3 py-3 whitespace-pre text-foreground" aria-hidden="true">
									{#each highlightedLines as line, index}
										<div style={editorLineStyle} class={`min-w-max ${index + 1 === rconLineInSelectedFile ? "bg-amber-400/15" : index + 1 === rconlogLineInSelectedFile ? "bg-emerald-400/15" : ""}`}>
											{@html line}
										</div>
									{/each}
								</div>
								<textarea
									bind:this={editorElement}
									bind:value={editorContent}
									disabled={!selectedFile || saving || busy}
									spellcheck="false"
									wrap="off"
									onkeydown={handleEditorKeydown}
									style={`height: ${editorContentHeight}; line-height: 20px;`}
									class="absolute inset-0 w-full min-w-full resize-none overflow-hidden whitespace-pre bg-transparent px-3 py-3 text-transparent caret-foreground outline-none placeholder:text-muted-foreground selection:bg-primary/35 disabled:cursor-not-allowed disabled:opacity-60"
									placeholder="Select a .cfg file to edit..."
									title="Server config editor"
								></textarea>
							</div>
						</div>
					</div>
					{/if}
					{#if externalFile}
						<div class="space-y-3 border-y border-amber-400/30 py-4">
							<p class="text-sm font-medium text-amber-400">This file changed on disk. Saving is paused.</p>
							<ConfigDiff before={externalFile.content} after={editorContent} beforeLabel="Current disk version" afterLabel="Your draft" />
							<label class="flex items-center gap-2 text-xs"><Checkbox bind:checked={externalReviewed} />I reviewed the external changes and my draft.</label>
							<div class="flex flex-wrap gap-2">
								<Button size="sm" variant="outline" onclick={() => reloadExternal()}><RefreshCwIcon />Reload file and discard draft</Button>
								<Button size="sm" variant="outline" disabled={!externalReviewed} onclick={() => reloadExternal(true)}>Keep reviewed draft</Button>
							</div>
						</div>
					{/if}
					{#if historyRequest && selectedFile}
						{#key selectedFile.path}
							<ConfigHistoryPanel request={historyRequest} currentContent={selectedFile.content} hasDraft={dirty} disabled={busy || saving || Boolean(externalFile)} onRestored={(file) => { acceptFile(file); notice = `${file.name} restored. The previous content is in history.`; noticeLevel = "success"; }} onBusy={(value) => saving = value} />
						{/key}
					{/if}
				</Card.Content>
			</Card.Root>
		</div>

		{#if serverCfgSelected}
			<Card.Root class="overflow-hidden rounded-sm border-border bg-card shadow-sm">
				<Card.Header class="border-b border-border pb-4">
					<div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
						<div>
							<Card.Title>RCON Autofill</Card.Title>
							<Card.Description>Add the recommended RCON setup lines and common command snippets to `server.cfg`.</Card.Description>
						</div>
						<Button variant="outline" onclick={autofillRconConfig} title="Add missing RCON setup lines to server.cfg">
							<ListPlusIcon />
							Add RCON Setup
						</Button>
					</div>
				</Card.Header>
				<Card.Content class="space-y-3">
					<div class="grid gap-2 md:grid-cols-2 xl:grid-cols-4">
						{#each rconCommands as item}
							<button
								type="button"
								class="rounded-sm border border-border bg-background/60 px-3 py-2 text-left transition-colors hover:bg-accent"
								onclick={() => autofillCommand(item.command)}
								title={`Add ${item.command} to server.cfg`}
							>
								<span class="block font-mono text-xs text-foreground">{item.command}</span>
								<span class="mt-1 block text-xs leading-5 text-muted-foreground">{item.description}</span>
							</button>
						{/each}
					</div>
				</Card.Content>
			</Card.Root>

			<Card.Root class="overflow-hidden rounded-sm border-border bg-card shadow-sm">
				<Card.Header class="border-b border-border pb-4">
					<Card.Title>Popular server.cfg Values</Card.Title>
					<Card.Description>Adjust common server settings here; the editor above stays as the source of truth.</Card.Description>
				</Card.Header>
				<Card.Content class="space-y-4">
					<div class="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
						<label class="grid gap-2">
							<span class="text-xs font-medium text-muted-foreground">Server Name</span>
							<Input value={popularCfgValues.hostname} onchange={(event) => updatePopularValue("sv_hostname", event)} placeholder="My FiveM Server" class="rounded-sm font-mono text-xs" />
						</label>
						<label class="grid gap-2">
							<span class="text-xs font-medium text-muted-foreground">Max Clients</span>
							<Input value={popularCfgValues.maxClients} onchange={(event) => updatePopularValue("sv_maxclients", event, false)} placeholder="48" class="rounded-sm font-mono text-xs" />
						</label>
						<label class="grid gap-2">
							<span class="text-xs font-medium text-muted-foreground">Project Name</span>
							<Input value={popularCfgValues.projectName} onchange={(event) => updatePopularValue("sets sv_projectName", event)} placeholder="FXServer Roleplay" class="rounded-sm font-mono text-xs" />
						</label>
						<label class="grid gap-2">
							<span class="text-xs font-medium text-muted-foreground">Project Description</span>
							<Input
								value={popularCfgValues.projectDescription}
								onchange={(event) => updatePopularValue("sets sv_projectDesc", event)}
								placeholder="A FiveM roleplay server"
								class="rounded-sm font-mono text-xs"
							/>
						</label>
						<label class="grid gap-2">
							<span class="text-xs font-medium text-muted-foreground">License Key</span>
							<Input value={popularCfgValues.licenseKey} onchange={(event) => updatePopularValue("sv_licenseKey", event)} placeholder="cfxk_..." class="rounded-sm font-mono text-xs" />
						</label>
						<label class="grid gap-2">
							<span class="text-xs font-medium text-muted-foreground">MySQL Connection String</span>
							<Input
								value={popularCfgValues.mysqlConnectionString || dbConnectionString}
								onchange={(event) => updatePopularValue("set mysql_connection_string", event)}
								placeholder="mysql://user:pass@localhost:3306/db"
								class="rounded-sm font-mono text-xs"
							/>
						</label>
					</div>
					<div class="flex flex-wrap gap-2">
						<Button variant="outline" onclick={setDbConnectionStringInCfg} disabled={!dbConnectionString} title="Use the validated database connection string">
							<ClipboardIcon />
							Set Validated DB String
						</Button>
						<Button variant="outline" onclick={autofillRconConfig} title="Add missing RCON setup lines to server.cfg">
							<ListPlusIcon />
							Ensure RCON
						</Button>
					</div>
				</Card.Content>
			</Card.Root>
		{/if}

		{#if serverCfgSelected || permissionsCfgSelected}
			<Card.Root class="overflow-hidden rounded-sm border-border bg-card shadow-sm">
				<Card.Header class="border-b border-border pb-4">
					<Card.Title>Permissions Helpers</Card.Title>
					<Card.Description>Add permission lines for the selected config file without hunting through the editor.</Card.Description>
				</Card.Header>
				<Card.Content class="space-y-4">
					{#if serverCfgSelected}
						<div class="grid gap-3 md:grid-cols-[minmax(0,1.2fr)_minmax(0,0.8fr)_minmax(0,0.8fr)_auto] md:items-end">
							<label class="grid gap-2">
								<span class="text-xs font-medium text-muted-foreground">Player Identifier</span>
								<Input bind:value={serverPrincipalIdentifier} placeholder="identifier.fivem:12345678" class="rounded-sm font-mono text-xs" />
							</label>
							<label class="grid gap-2">
								<span class="text-xs font-medium text-muted-foreground">Group</span>
								<Input bind:value={serverPrincipalGroup} placeholder="group.admin" class="rounded-sm font-mono text-xs" />
							</label>
							<label class="grid gap-2">
								<span class="text-xs font-medium text-muted-foreground">Comment</span>
								<Input bind:value={serverPrincipalComment} placeholder="Player name" class="rounded-sm font-mono text-xs" />
							</label>
							<Button onclick={addServerPrincipal} title="Add this principal to server.cfg">
								<ListPlusIcon />
								Add Principal
							</Button>
						</div>
					{/if}
					{#if permissionsCfgSelected}
						<div class="space-y-4">
							<div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_10rem_auto] md:items-end">
								<label class="grid gap-2">
									<span class="text-xs font-medium text-muted-foreground">Principal</span>
									<Input bind:value={acePrincipal} placeholder="group.admin" class="rounded-sm font-mono text-xs" />
								</label>
								<label class="grid gap-2">
									<span class="text-xs font-medium text-muted-foreground">Permission</span>
									<Input bind:value={aceObject} placeholder="command" class="rounded-sm font-mono text-xs" />
								</label>
								<label class="grid gap-2">
									<span class="text-xs font-medium text-muted-foreground">Access</span>
									<Input bind:value={aceAccess} placeholder="allow" class="rounded-sm font-mono text-xs" />
								</label>
								<Button onclick={addPermissionsAce} title="Add this ACE rule to permissions.cfg">
									<ListPlusIcon />
									Add ACE
								</Button>
							</div>
							<div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] md:items-end">
								<label class="grid gap-2">
									<span class="text-xs font-medium text-muted-foreground">Child Group</span>
									<Input bind:value={inheritanceChild} placeholder="group.admin" class="rounded-sm font-mono text-xs" />
								</label>
								<label class="grid gap-2">
									<span class="text-xs font-medium text-muted-foreground">Inherits From</span>
									<Input bind:value={inheritanceParent} placeholder="group.mod" class="rounded-sm font-mono text-xs" />
								</label>
								<Button variant="outline" onclick={addPermissionsInheritance} title="Add this group inheritance to permissions.cfg">
									<ListPlusIcon />
									Add Inheritance
								</Button>
							</div>
							<div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_10rem_auto] md:items-end">
								<label class="grid gap-2">
									<span class="text-xs font-medium text-muted-foreground">Resource</span>
									<Input bind:value={resourceName} placeholder="resource_name" class="rounded-sm font-mono text-xs" />
								</label>
								<label class="grid gap-2">
									<span class="text-xs font-medium text-muted-foreground">Permission</span>
									<Input bind:value={resourceAceObject} placeholder="command" class="rounded-sm font-mono text-xs" />
								</label>
								<label class="grid gap-2">
									<span class="text-xs font-medium text-muted-foreground">Access</span>
									<Input bind:value={resourceAceAccess} placeholder="allow" class="rounded-sm font-mono text-xs" />
								</label>
								<Button variant="outline" onclick={addResourceAce} title="Add this resource ACE to permissions.cfg">
									<ListPlusIcon />
									Add Resource ACE
								</Button>
							</div>
						</div>
					{/if}
				</Card.Content>
			</Card.Root>
		{/if}
	{/if}
</section>
