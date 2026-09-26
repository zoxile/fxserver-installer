import { invoke } from "@tauri-apps/api/core";
import type { ConfigFileRequest } from "./configHistory";

export interface ConfigValidationIssue {
    code: string;
    severity: "error" | "warning";
    file: string;
    line: number | null;
    message: string;
}

export interface ConfigValidationReport {
    issues: ConfigValidationIssue[];
    revision: string;
    fileCount: number;
}

export interface ConfigChangeReview {
    target: ConfigFileRequest;
    before: string;
    after: string;
    report: ConfigValidationReport;
}

export function validateConfigChange(request: ConfigFileRequest, content: string): Promise<ConfigValidationReport> {
    // Do not put draft text or raw IPC failures into the task log.
    return invoke<ConfigValidationReport>("validate_config_change", { request: { ...request }, content });
}

export function reviewMatches(review: ConfigChangeReview, target: ConfigFileRequest | null, before: string, after: string): boolean {
    return Boolean(target && review.target.txDataPath === target.txDataPath && review.target.profile === target.profile
        && review.target.path === target.path && review.before === before && review.after === after);
}

export function canSaveReview(report: ConfigValidationReport, diffReviewed: boolean, warningsReviewed: boolean): boolean {
    return Boolean(report.revision) && diffReviewed && !report.issues.some((issue) => issue.severity === "error")
        && (warningsReviewed || !report.issues.some((issue) => issue.severity === "warning"));
}
