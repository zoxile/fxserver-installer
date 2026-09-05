import { taskInvoke as invoke } from "$lib/core/tasks.svelte";
import { getInstallPath } from "$lib/core/paths.svelte";
import { log } from "$lib/core/logger.svelte";

const artifactsApiUrl = "https://artifacts.jgscripts.com/jsonv2";
const devArtifactsApiUrl = "/api/jg-artifacts/jsonv2";
const sourceUrl = "https://artifacts.jgscripts.com/";

export interface ArtifactIssue {
	artifact: string;
	reason: string;
}

interface JgArtifactResponse {
	recommendedArtifact: string;
	windowsDownloadLink: string;
	linuxDownloadLink?: string;
	brokenArtifacts: ArtifactIssue[];
}

export interface ArtifactMetadata {
	recommendedArtifact: string;
	windowsDownloadLink: string;
	brokenArtifacts: ArtifactIssue[];
	fetchedAt: string;
	sourceUrl: string;
}

export interface ArtifactInstallResult {
	version: string;
	destination: string;
	markerPath: string;
}

export interface InstalledArtifactInfo {
	installed: boolean;
	version?: string | null;
	destination: string;
	markerPath: string;
	citizenServerImplPath?: string | null;
	fileVersion?: string | null;
	productVersion?: string | null;
	hasFxserverExecutable: boolean;
	detectionSource: "marker" | "executable" | "none" | string;
}

export type ArtifactUpdateUrgency = "none" | "recommended" | "needed" | "unknown";

export interface ArtifactHealthStatus {
	urgency: ArtifactUpdateUrgency;
	label: string;
	description: string;
	currentVersion?: string | null;
	recommendedVersion?: string | null;
	issue?: ArtifactIssue | null;
}

function hasTauriRuntime() {
	return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function errorMessage(error: unknown) {
	return error instanceof Error ? error.message : String(error);
}

function ensureString(value: unknown, label: string) {
	if (typeof value !== "string" || !value.trim()) {
		throw new Error(`JG Scripts artifact response is missing ${label}.`);
	}

	return value.trim();
}

function normalizeIssues(value: unknown): ArtifactIssue[] {
	if (!Array.isArray(value)) return [];

	return value
		.map((entry) => {
			if (!entry || typeof entry !== "object") return null;
			const artifact = "artifact" in entry ? String(entry.artifact ?? "").trim() : "";
			const reason = "reason" in entry ? String(entry.reason ?? "").trim() : "";
			if (!artifact || !reason) return null;
			return { artifact, reason };
		})
		.filter((entry): entry is ArtifactIssue => Boolean(entry));
}

function normalizeMetadata(data: JgArtifactResponse): ArtifactMetadata {
	return {
		recommendedArtifact: ensureString(data.recommendedArtifact, "recommendedArtifact"),
		windowsDownloadLink: ensureString(data.windowsDownloadLink, "windowsDownloadLink"),
		brokenArtifacts: normalizeIssues(data.brokenArtifacts),
		fetchedAt: new Date().toISOString(),
		sourceUrl,
	};
}

export async function fetchArtifactMetadata() {
	log("Fetching Windows artifact metadata from JG Scripts.", { scope: "artifacts.api" });

	try {
		if (hasTauriRuntime()) {
			const data = await invoke<JgArtifactResponse>("get_windows_artifact_metadata");
			const metadata = normalizeMetadata(data);
			log(`JG Scripts recommends Windows artifact ${metadata.recommendedArtifact}.`, {
				level: "success",
				scope: "artifacts.api",
				detail: `${metadata.brokenArtifacts.length} known problematic artifact ranges loaded.`,
			});
			return metadata;
		}

		const response = await fetch(devArtifactsApiUrl, {
			headers: {
				Accept: "application/json",
			},
		});

		if (!response.ok) {
			throw new Error(`JG Scripts returned ${response.status} ${response.statusText}.`);
		}

		const metadata = normalizeMetadata((await response.json()) as JgArtifactResponse);
		log(`JG Scripts recommends Windows artifact ${metadata.recommendedArtifact}.`, {
			level: "success",
			scope: "artifacts.api",
			detail: `${metadata.brokenArtifacts.length} known problematic artifact ranges loaded.`,
		});
		return metadata;
	} catch (error) {
		log("Failed to fetch artifact metadata from JG Scripts.", {
			level: "error",
			scope: "artifacts.api",
			detail: `${errorMessage(error)} (${artifactsApiUrl})`,
		});
		throw error;
	}
}

export function artifactIsFlagged(version: string, issues: ArtifactIssue[]) {
	return Boolean(findArtifactIssue(version, issues));
}

export function findArtifactIssue(version: string, issues: ArtifactIssue[]) {
	const numericVersion = Number.parseInt(version, 10);
	if (!Number.isFinite(numericVersion)) return null;

	return (
		issues.find((issue) => {
			const [start, end] = issue.artifact.split("-").map((part) => Number.parseInt(part, 10));
			if (!Number.isFinite(start)) return false;
			if (!Number.isFinite(end)) return numericVersion === start;
			return numericVersion >= start && numericVersion <= end;
		}) ?? null
	);
}

function artifactNumber(version?: string | null) {
	if (!version) return null;
	const parsed = Number.parseInt(version, 10);
	return Number.isFinite(parsed) ? parsed : null;
}

export function getArtifactHealthStatus(metadata: ArtifactMetadata | null, installed: InstalledArtifactInfo | null): ArtifactHealthStatus {
	if (!metadata) {
		return {
			urgency: "unknown",
			label: "Checking artifacts",
			description: "Artifact metadata has not loaded yet.",
			currentVersion: installed?.version,
			recommendedVersion: null,
		};
	}

	if (!installed?.installed) {
		return {
			urgency: "unknown",
			label: "Not installed",
			description: "Choose an FXServer folder and install the recommended Windows artifact.",
			currentVersion: null,
			recommendedVersion: metadata.recommendedArtifact,
		};
	}

	if (!installed.version) {
		return {
			urgency: "unknown",
			label: "Installed build unknown",
			description: "FXServer files were found, but this app has not recorded the artifact version yet.",
			currentVersion: null,
			recommendedVersion: metadata.recommendedArtifact,
		};
	}

	const issue = findArtifactIssue(installed.version, metadata.brokenArtifacts);
	if (issue) {
		return {
			urgency: "needed",
			label: "Update needed",
			description: `Current artifact is reported problematic: ${issue.reason}`,
			currentVersion: installed.version,
			recommendedVersion: metadata.recommendedArtifact,
			issue,
		};
	}

	const current = artifactNumber(installed.version);
	const recommended = artifactNumber(metadata.recommendedArtifact);
	const recommendedHealthy = !artifactIsFlagged(metadata.recommendedArtifact, metadata.brokenArtifacts);

	if (current != null && recommended != null && recommended > current && recommendedHealthy) {
		return {
			urgency: "recommended",
			label: "Update recommended",
			description: "A newer healthy Windows artifact is available.",
			currentVersion: installed.version,
			recommendedVersion: metadata.recommendedArtifact,
		};
	}

	return {
		urgency: "none",
		label: "Up to date",
		description: "Installed artifact has no reported issue and matches the current recommendation.",
		currentVersion: installed.version,
		recommendedVersion: metadata.recommendedArtifact,
	};
}

export async function getInstalledWindowsArtifactInfo(destination = getInstallPath()) {
	if (!destination) {
		return null;
	}

	if (!hasTauriRuntime()) {
		log("Installed artifact check skipped outside the desktop runtime.", { level: "debug", scope: "artifacts.install" });
		return null;
	}

	try {
		const info = await invoke<InstalledArtifactInfo>("get_installed_windows_artifact_info", { destination });
		log(info.installed ? `Installed artifact detected${info.version ? `: ${info.version}` : "."}` : "No installed artifact marker found.", {
			level: info.installed ? "success" : "debug",
			scope: "artifacts.install",
			detail: destination,
		});
		return info;
	} catch (error) {
		log("Installed artifact check failed.", {
			level: "error",
			scope: "artifacts.install",
			detail: errorMessage(error),
		});
		throw error;
	}
}

export async function installWindowsArtifact(metadata: ArtifactMetadata, destination = getInstallPath()) {
	if (!hasTauriRuntime()) {
		log("Artifact install blocked outside the desktop runtime.", { level: "warn", scope: "artifacts.install" });
		throw new Error("Artifact installation is available in the Tauri desktop app.");
	}

	if (!destination) {
		throw new Error("Choose an install folder before installing artifacts.");
	}

	log(`Installing Windows FXServer artifact ${metadata.recommendedArtifact}.`, {
		scope: "artifacts.install",
		detail: destination,
	});

	try {
		const result = await invoke<ArtifactInstallResult>("install_windows_artifact", {
			request: {
				version: metadata.recommendedArtifact,
				url: metadata.windowsDownloadLink,
				destination,
			},
		});
		log(`Windows FXServer artifact ${result.version} installed.`, {
			level: "success",
			scope: "artifacts.install",
			detail: result.destination,
		});
		return result;
	} catch (error) {
		log("Windows FXServer artifact install failed.", {
			level: "error",
			scope: "artifacts.install",
			detail: errorMessage(error),
		});
		throw error;
	}
}
