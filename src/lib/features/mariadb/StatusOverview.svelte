<script lang="ts">
	import PlayIcon from "@lucide/svelte/icons/play";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
	import SquareIcon from "@lucide/svelte/icons/square";
	import ServerIcon from "@lucide/svelte/icons/server";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import type { MariaDBStatus } from "$lib/modules/mariadb";

	type Props = {
		status: MariaDBStatus | null;
		busy: boolean;
		onRefresh: () => void;
		onStart: () => void;
		onStop: () => void;
		onRestart: () => void;
	};

	let { status, busy, onRefresh, onStart, onStop, onRestart }: Props = $props();
</script>

<Card.Root class="h-full rounded-md border-border bg-card shadow-sm">
	<Card.Header class="border-b border-border pb-4">
		<div class="flex items-start justify-between gap-4">
			<div class="flex min-w-0 items-center gap-3">
				<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
					<ServerIcon class="size-5" />
				</div>
				<div class="min-w-0">
					<Card.Title>Service Status</Card.Title>
					<Card.Description>Detected MariaDB installation and Windows service state.</Card.Description>
				</div>
			</div>
			<span
				class={[
					"rounded-sm px-2.5 py-1 text-xs font-semibold",
					status?.running ? "bg-primary/10 text-primary" : "bg-destructive/10 text-destructive",
				]}
			>
				{status?.running ? "Running" : "Stopped"}
			</span>
		</div>
	</Card.Header>

	<Card.Content class="space-y-5">
		<div class="grid gap-3 sm:grid-cols-2">
			<div class="rounded-sm border border-border bg-background p-3">
				<p class="text-xs text-muted-foreground">Installed</p>
				<p class="mt-1 font-semibold">{status?.installed ? "Yes" : "No"}</p>
			</div>
			<div class="rounded-sm border border-border bg-background p-3">
				<p class="text-xs text-muted-foreground">Service</p>
				<p class="mt-1 truncate font-semibold">{status?.serviceDisplayName || status?.serviceName || "Not found"}</p>
			</div>
			<div class="rounded-sm border border-border bg-background p-3">
				<p class="text-xs text-muted-foreground">Version</p>
				<p class="mt-1 truncate font-semibold">{status?.version || "Unknown"}</p>
			</div>
			<div class="rounded-sm border border-border bg-background p-3">
				<p class="text-xs text-muted-foreground">Install Path</p>
				<p class="mt-1 truncate font-semibold">{status?.installPath || "Unknown"}</p>
			</div>
		</div>

		<div class="flex flex-wrap gap-2">
			<Button variant="outline" onclick={onRefresh} disabled={busy} title="Refresh MariaDB status">
				<RefreshCwIcon />
				Refresh
			</Button>
			<Button variant="secondary" onclick={onStart} disabled={busy || !status?.installed} title="Start service">
				<PlayIcon />
				Start
			</Button>
			<Button variant="secondary" onclick={onStop} disabled={busy || !status?.installed} title="Stop service">
				<SquareIcon />
				Stop
			</Button>
			<Button variant="secondary" onclick={onRestart} disabled={busy || !status?.installed} title="Restart service">
				<RotateCwIcon />
				Restart
			</Button>
		</div>
	</Card.Content>
</Card.Root>
