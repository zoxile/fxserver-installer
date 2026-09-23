<script lang="ts">
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import SaveIcon from "@lucide/svelte/icons/save";
	import ShieldIcon from "@lucide/svelte/icons/shield";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import UsersIcon from "@lucide/svelte/icons/users";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import PasswordInput from "$lib/components/ui/password-input.svelte";
	import * as Select from "$lib/components/ui/select/index.js";
	import type { MariaDBUser, MariaDBUserAccess } from "$lib/modules/mariadb";
	import AuthenticationCompatibility from "./AuthenticationCompatibility.svelte";

	type EditableUser = {
		username: string;
		host: string;
		password: string;
		database: string;
		privileges: string;
	};

	type Props = {
		busy: boolean;
		credentialsReady: boolean;
		users: MariaDBUser[];
		selectedUser: MariaDBUser | null;
		selectedAccess: MariaDBUserAccess | null;
		editingUser: EditableUser | null;
		nativePassword?: boolean;
		databases: string[];
		onRefresh: () => void;
		onEdit: (user: MariaDBUser) => void;
		onSave: () => void;
		onDelete: (user: MariaDBUser) => void;
	};

	let { busy, credentialsReady, users, selectedUser, selectedAccess, editingUser = $bindable(), nativePassword = $bindable(false), databases, onRefresh, onEdit, onSave, onDelete }: Props = $props();
	const databaseOptions = $derived(databases.map((database) => ({ value: database, label: database })));
</script>

<Card.Root class="h-full min-h-136 min-w-0 rounded-md border-border bg-card shadow-sm">
	<Card.Header class="shrink-0 border-b border-border pb-4">
		<div class="flex items-start justify-between gap-3">
			<div class="flex items-center gap-3">
				<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
					<UsersIcon class="size-5" />
				</div>
				<div>
					<Card.Title>Existing Users</Card.Title>
					<Card.Description>Review, edit, inspect access, and remove accounts already present in MariaDB.</Card.Description>
				</div>
			</div>
			<Button
				variant="outline"
				size="icon"
				onclick={onRefresh}
				disabled={busy || !credentialsReady}
				aria-label="Refresh existing MariaDB users"
				title={credentialsReady ? "Refresh existing MariaDB users" : "Apply valid admin credentials before refreshing users"}
			>
				<RefreshCwIcon class={busy ? "animate-spin" : undefined} />
			</Button>
		</div>
	</Card.Header>

	<Card.Content class="flex min-h-0 min-w-0 flex-1 flex-col gap-4 @container">
		{#if users.length === 0}
			<div class="rounded-sm border border-dashed border-border bg-background/60 p-4 text-sm text-muted-foreground">
				{credentialsReady ? "No users loaded. Refresh to fetch accounts from MariaDB." : "Apply valid admin credentials before loading users."}
			</div>
		{:else}
			<!-- The list fills the card without letting its rows determine the grid track height. -->
			<div class="relative min-h-72 flex-1">
				<!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard users need to scroll the bounded account list.) -->
				<div role="region" aria-label="MariaDB accounts" tabindex="0" class="absolute inset-0 overflow-y-auto overscroll-contain rounded-sm p-1 focus-visible:outline-2 focus-visible:outline-ring">
					<ul class="space-y-2">
						{#each users as user}
							{@const isSelected = selectedUser?.username === user.username && selectedUser?.host === user.host}
							<li
								class={[
									"group flex items-center justify-between gap-3 rounded-sm border px-3 py-2 transition-[background-color,border-color] duration-500 ease-[cubic-bezier(0.22,1,0.36,1)] hover:border-primary/35 hover:bg-accent/40",
									isSelected ? "border-primary/45 bg-accent/50" : "border-border bg-background/70",
								]}
							>
								<button
									class="min-w-0 flex-1 rounded-sm text-left focus-visible:outline-2 focus-visible:outline-ring disabled:cursor-not-allowed disabled:opacity-60"
									disabled={busy || !credentialsReady}
									onclick={() => onEdit(user)}
									aria-label={`Edit ${user.username || "(anonymous)"}@${user.host}`}
									title={`Click to edit ${user.username}@${user.host}`}
								>
									<p class="truncate text-sm font-medium text-foreground">{user.username || "(anonymous)"}@{user.host}</p>
									<p class="mt-1 flex items-start gap-2 text-xs text-muted-foreground">
										<ShieldIcon class="mt-0.5 size-3.5 shrink-0" />
										<span class="min-w-0 wrap-anywhere">{user.plugin || "plugin unknown"} - locked {user.locked || "unknown"}</span>
									</p>
								</button>
								<Button variant="destructive" size="icon" onclick={() => onDelete(user)} disabled={busy || !credentialsReady} aria-label={`Delete ${user.username || "(anonymous)"}@${user.host}`} title={`Delete ${user.username}@${user.host}`}>
									<Trash2Icon />
								</Button>
							</li>
						{/each}
					</ul>
				</div>
			</div>
		{/if}

		{#if selectedUser}
			<section aria-label="Selected account access" class="min-w-0 shrink-0 space-y-3 border-t border-border pt-4 wrap-anywhere">
				<div>
					<p class="text-sm font-medium text-foreground">Access details for {selectedUser.username || "(anonymous)"}@{selectedUser.host}</p>
					<p class="mt-1 text-xs text-muted-foreground">Grants, schema-level privileges, and table-level privileges from the selected admin credentials.</p>
				</div>

				{#if !selectedAccess}
					<p class="rounded-sm border border-dashed border-border p-3 text-xs text-muted-foreground">Select or refresh the user to load access details.</p>
				{:else}
					<div class="grid gap-3 @lg:grid-cols-2">
						<div class="min-w-0 space-y-2">
							<p class="text-xs font-medium text-muted-foreground">Raw Grants</p>
							<div class="max-h-40 space-y-2 overflow-auto rounded-sm border border-border bg-card p-2">
								{#each selectedAccess.grants as grant}
									<code class="block whitespace-pre-wrap wrap-break-word text-xs text-foreground">{grant}</code>
								{:else}
									<p class="text-xs text-muted-foreground">No raw grants returned.</p>
								{/each}
							</div>
						</div>

						<div class="min-w-0 space-y-2">
							<p class="text-xs font-medium text-muted-foreground">Database Access</p>
							<div class="max-h-40 space-y-2 overflow-auto rounded-sm border border-border bg-card p-2">
								{#each selectedAccess.schemaPrivileges as privilege}
									<div class="rounded-sm bg-background/70 px-2 py-1.5 text-xs">
										<span class="font-medium text-foreground">{privilege.database}</span>
										<span class="text-muted-foreground"> - {privilege.privilege} - grantable {privilege.grantable}</span>
									</div>
								{:else}
									<p class="text-xs text-muted-foreground">No schema-level privileges found.</p>
								{/each}
							</div>
						</div>
					</div>

					<div class="space-y-2">
						<p class="text-xs font-medium text-muted-foreground">Table Access</p>
						<div class="max-h-44 overflow-auto rounded-sm border border-border bg-card">
							{#each selectedAccess.tablePrivileges as privilege}
								<div class="grid gap-2 border-b border-border px-3 py-2 text-xs last:border-b-0 @lg:grid-cols-[1fr_1fr_1fr_auto]">
									<span class="font-medium text-foreground">{privilege.database}</span>
									<span class="text-muted-foreground">{privilege.table || "*"}</span>
									<span class="text-muted-foreground">{privilege.privilege}</span>
									<span class="text-muted-foreground">grantable {privilege.grantable}</span>
								</div>
							{:else}
								<p class="p-3 text-xs text-muted-foreground">No table-level privileges found.</p>
							{/each}
						</div>
					</div>
				{/if}
			</section>
		{/if}

		{#if editingUser}
			<form aria-label="Edit database user" class="min-w-0 shrink-0 space-y-4 border-t border-border pt-4" onsubmit={(event) => { event.preventDefault(); if (!busy && credentialsReady) onSave(); }}>
				<div>
					<p class="text-sm font-medium wrap-anywhere text-foreground">Edit {editingUser.username}@{editingUser.host}</p>
					<p class="mt-1 text-xs text-muted-foreground">{nativePassword ? "Enter a password to replace this account's authentication." : "Leave password empty to keep the current password."}</p>
				</div>
				<div class="grid gap-4 @sm:grid-cols-2">
					<label class="grid min-w-0 gap-2">
						<span class="text-xs font-medium text-muted-foreground">Password</span>
						<PasswordInput bind:value={editingUser.password} disabled={!credentialsReady} placeholder="New password or blank" title="New password for this MariaDB user." />
					</label>
					<label class="grid min-w-0 gap-2">
						<span class="text-xs font-medium text-muted-foreground">Database</span>
						<Select.Root bind:value={editingUser.database} type="single" items={databaseOptions} disabled={!credentialsReady || !databaseOptions.length}>
							<Select.Trigger title="Choose database to grant permissions on" class="w-full min-w-0 rounded-sm font-mono text-xs">
								<span class="truncate">{editingUser.database || "Choose database"}</span>
							</Select.Trigger>
							<Select.Content class="rounded-sm">
								{#if databaseOptions.length}
									{#each databaseOptions as option}
										<Select.Item value={option.value} label={option.label}>
											{option.label}
										</Select.Item>
									{/each}
								{:else}
									<Select.Item value="" label="No databases loaded" disabled>No databases loaded</Select.Item>
								{/if}
							</Select.Content>
						</Select.Root>
					</label>
					<label class="grid min-w-0 gap-2 @sm:col-span-2">
						<span class="text-xs font-medium text-muted-foreground">Permissions</span>
						<Input bind:value={editingUser.privileges} disabled={!credentialsReady} placeholder="SELECT, INSERT, UPDATE or ALL PRIVILEGES" title="Comma-separated permissions to grant." />
					</label>
				</div>
				<AuthenticationCompatibility bind:nativePassword disabled={busy || !credentialsReady} />
				<Button type="submit" disabled={busy || !credentialsReady} title="Save edits for this MariaDB user">
					<SaveIcon />
					Save Changes
				</Button>
			</form>
		{/if}
	</Card.Content>
</Card.Root>
