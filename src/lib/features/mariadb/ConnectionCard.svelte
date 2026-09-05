<script lang="ts">
	import KeyRoundIcon from "@lucide/svelte/icons/key-round";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import PasswordInput from "$lib/components/ui/password-input.svelte";
	import type { MariaDBCredentials } from "$lib/modules/mariadb";

	type Props = {
		busy: boolean;
		credentialsReady: boolean;
		connectionError: string;
		credentials: MariaDBCredentials;
		stretch?: boolean;
		onApply: () => void;
	};

	let { busy, credentialsReady, connectionError, credentials = $bindable(), stretch = true, onApply }: Props = $props();
</script>

<Card.Root class={`${stretch ? "h-full" : ""} rounded-md border-border bg-card shadow-sm`}>
	<Card.Header class="border-b border-border pb-4">
		<div class="flex items-center gap-3">
			<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
				<KeyRoundIcon class="size-5" />
			</div>
			<div>
				<Card.Title>Connection</Card.Title>
				<Card.Description>Admin credentials used for service queries and user changes.</Card.Description>
			</div>
		</div>
	</Card.Header>

	<Card.Content class="space-y-4">
		<div class="grid gap-4 sm:grid-cols-2">
			<label class="grid gap-2">
				<span class="text-xs font-medium text-muted-foreground">Host</span>
				<Input bind:value={credentials.host} disabled={busy} placeholder="127.0.0.1" title="MariaDB host for admin actions." />
			</label>
			<label class="grid gap-2">
				<span class="text-xs font-medium text-muted-foreground">Port</span>
				<Input type="number" bind:value={credentials.port} disabled={busy} placeholder="3306" title="MariaDB port for admin actions." />
			</label>
			<label class="grid gap-2">
				<span class="text-xs font-medium text-muted-foreground">Admin Username</span>
				<Input bind:value={credentials.username} disabled={busy} placeholder="root" title="Admin username used to connect to MariaDB." />
			</label>
			<label class="grid gap-2">
				<span class="text-xs font-medium text-muted-foreground">Admin Password</span>
				<PasswordInput bind:value={credentials.password} disabled={busy} placeholder="Root/admin password" title="Admin password used to connect to MariaDB." />
			</label>
			<label class="grid gap-2 sm:col-span-2">
				<span class="text-xs font-medium text-muted-foreground">Default Database</span>
				<Input bind:value={credentials.database} disabled={busy} placeholder="Optional default schema" title="Optional database to use when running queries." />
			</label>
		</div>
		<Button class="w-full" onclick={onApply} disabled={busy} title="Apply admin credentials and refresh MariaDB status, users, and selected user details">
			<RefreshCwIcon class={busy ? "animate-spin" : undefined} />
			Change Credentials
		</Button>
		{#if connectionError}
			<p class="rounded-sm border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">{connectionError}</p>
		{:else if credentialsReady}
			<p class="rounded-sm border border-emerald-400/25 bg-emerald-400/10 px-3 py-2 text-xs text-emerald-200">Credentials validated.</p>
		{:else}
			<p class="rounded-sm border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">Apply credentials to validate the MariaDB connection.</p>
		{/if}
	</Card.Content>
</Card.Root>
