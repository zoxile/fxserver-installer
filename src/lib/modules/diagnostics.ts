import { taskInvoke } from "$lib/core/tasks.svelte";
import type { MariaDBCredentials } from "$lib/modules/mariadb";

export type DiagnosticSeverity = "error" | "warning" | "info" | "pass";

export interface PreflightRequest {
	artifactPath: string;
	txDataPath: string;
	profile: string;
	credentials?: MariaDBCredentials | null;
	checkPorts?: boolean;
}

export interface DiagnosticCheck {
	category: string;
	code: string;
	severity: DiagnosticSeverity;
	title: string;
	detail: string;
	resource: string | null;
	file: string | null;
	line: number | null;
}

export interface PreflightReport {
	checkedAt: number;
	blocking: boolean;
	errorCount: number;
	warningCount: number;
	resourceCount: number;
	configCount: number;
	checks: DiagnosticCheck[];
}

export interface DiagnosticPreviewRequest {
	preflight: PreflightRequest;
	includeApplicationLog?: boolean;
	includeServerLog?: boolean;
}

export interface DiagnosticPreview {
	id: string;
	createdAt: number;
	expiresAt: number;
	entries: { name: string; content: string }[];
	totalBytes: number;
}

export function runPreflight(request: PreflightRequest): Promise<PreflightReport> {
	return taskInvoke("run_fxserver_preflight", { request });
}

export function previewDiagnosticExport(request: DiagnosticPreviewRequest): Promise<DiagnosticPreview> {
	return taskInvoke("preview_diagnostic_export", { request });
}

export function exportDiagnosticZip(previewId: string, path: string): Promise<{ path: string; sizeBytes: number }> {
	return taskInvoke("export_diagnostic_zip", { previewId, path });
}
