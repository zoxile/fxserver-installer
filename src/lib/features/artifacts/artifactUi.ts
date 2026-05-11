import type { ArtifactUpdateUrgency } from "$lib/modules/artifact";

export function artifactUrgencyClass(urgency: ArtifactUpdateUrgency) {
	return {
		needed: "border-red-400/30 bg-red-400/10 text-red-200",
		recommended: "border-amber-400/30 bg-amber-400/10 text-amber-200",
		none: "border-emerald-400/30 bg-emerald-400/10 text-emerald-200",
		unknown: "border-muted bg-muted/50 text-muted-foreground",
	}[urgency];
}

export function artifactUrgencyTextClass(urgency: ArtifactUpdateUrgency) {
	return {
		needed: "text-red-200",
		recommended: "text-amber-200",
		none: "text-emerald-200",
		unknown: "text-muted-foreground",
	}[urgency];
}
