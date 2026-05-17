<script lang="ts">
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import SaveIcon from "@lucide/svelte/icons/save";
	import ShieldIcon from "@lucide/svelte/icons/shield";
	import SquareMousePointerIcon from "@lucide/svelte/icons/square-mouse-pointer";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import UsersIcon from "@lucide/svelte/icons/users";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import type { MariaDBUser, MariaDBUserAccess } from "$lib/modules/mariadb";

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
		databases: string[];
		onRefresh: () => void;
		onEdit: (user: MariaDBUser) => void;
		onSave: () => void;
		onDelete: (user: MariaDBUser) => void;
	};

	let { busy, credentialsReady, users, selectedUser, selectedAccess, editingUser = $bindable(), databases, onRefresh, onEdit, onSave, onDelete }: Props = $props();
	const databaseOptions = $derived(databases.map((database) => ({ value: database, label: database })));
</script>

<Card.Root class="h-full min-h-136 rounded-md border-border bg-card shadow-sm">
	<Card.Header class="border-b border-border pb-4">
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
				title={credentialsReady ? "Refresh existing MariaDB users" : "Apply valid admin credentials before refreshing users"}
			>
				<RefreshCwIcon class={busy ? "animate-spin" : undefined} />
			</Button>
		</div>
	</Card.Header>

	<Card.Content class="space-y-4">
		{#if users.length === 0}
			<div class="rounded-sm border border-dashed border-border bg-background/60 p-4 text-sm text-muted-foreground">
				{credentialsReady ? "No users loaded. Refresh to fetch accounts from MariaDB." : "Apply valid admin credentials before loading users."}
			</div>
		{:else}
			<div class="max-h-72 space-y-2 overflow-y-auto pr-1">
				{#each users as user}
					{@const isSelected = selectedUser?.username === user.username && selectedUser?.host === user.host}
					<div
						class={[
							"group flex items-center justify-between gap-3 rounded-sm border px-3 py-2 transition-[background-color,border-color,transform] duration-150 hover:-translate-y-0.5 hover:border-primary/35 hover:bg-accent/40",
							isSelected ? "border-primary/45 bg-accent/50" : "border-border bg-background/70",
						]}
					>
						<button
							class="min-w-0 flex-1 text-left disabled:cursor-not-allowed disabled:opacity-60"
							disabled={!credentialsReady}
							onclick={() => onEdit(user)}
							title={`Click to edit ${user.username}@${user.host}`}
						>
							<p class="truncate text-sm font-medium text-foreground">{user.username || "(anonymous)"}@{user.host}</p>
							<p class="mt-1 flex items-center gap-2 truncate text-xs text-muted-foreground">
								<ShieldIcon class="size-3.5" />
								{user.plugin || "plugin unknown"} - locked {user.locked || "unknown"}
							</p>
							<p class="mt-1 flex items-center gap-1.5 text-xs text-muted-foreground opacity-70 transition-opacity group-hover:opacity-100">
								<SquareMousePointerIcon class="size-3.5" />
								Click to edit properties and inspect access
							</p>
						</button>
						<Button variant="destructive" size="icon" onclick={() => onDelete(user)} disabled={busy || !credentialsReady} title={`Delete ${user.username}@${user.host}`}>
							<Trash2Icon />
						</Button>
					</div>
				{/each}
			</div>
		{/if}

		{#if selectedUser}
			<div class="space-y-3 rounded-sm border border-border bg-background/70 p-4">
				<div>
					<p class="text-sm font-medium text-foreground">Access details for {selectedUser.username || "(anonymous)"}@{selectedUser.host}</p>
					<p class="mt-1 text-xs text-muted-foreground">Grants, schema-level privileges, and table-level privileges from the selected admin credentials.</p>
				</div>

				{#if !selectedAccess}
					<p class="rounded-sm border border-dashed border-border p-3 text-xs text-muted-foreground">Select or refresh the user to load access details.</p>
				{:else}
					<div class="grid gap-3 xl:grid-cols-2">
						<div class="space-y-2">
							<p class="text-xs font-medium text-muted-foreground">Raw Grants</p>
							<div class="max-h-40 space-y-2 overflow-auto rounded-sm border border-border bg-card p-2">
								{#each selectedAccess.grants as grant}
									<code class="block whitespace-pre-wrap wrap-break-word text-xs text-foreground">{grant}</code>
								{:else}
									<p class="text-xs text-muted-foreground">No raw grants returned.</p>
								{/each}
							</div>
						</div>

						<div class="space-y-2">
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
								<div class="grid gap-2 border-b border-border px-3 py-2 text-xs last:border-b-0 sm:grid-cols-[1fr_1fr_1fr_auto]">
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
			</div>
		{/if}

		{#if editingUser}
			<div class="space-y-4 rounded-sm border border-border bg-background/70 p-4">
				<div>
					<p class="text-sm font-medium text-foreground">Edit {editingUser.username}@{editingUser.host}</p>
					<p class="mt-1 text-xs text-muted-foreground">Leave password empty to keep the current password.</p>
				</div>
				<div class="grid gap-4 sm:grid-cols-2">
					<label class="grid gap-2">
						<span class="text-xs font-medium text-muted-foreground">Password</span>
						<Input type="password" bind:value={editingUser.password} disabled={!credentialsReady} placeholder="New password or blank" title="New password for this MariaDB user." />
					</label>
					<label class="grid gap-2">
						<span class="text-xs font-medium text-muted-foreground">Database</span>
						<Select.Root bind:value={editingUser.database} type="single" items={databaseOptions} disabled={!credentialsReady || !databaseOptions.length}>
							<Select.Trigger title="Choose database to grant permissions on" class="w-full rounded-sm font-mono text-xs">
								{editingUser.database || "Choose database"}
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
					<label class="grid gap-2 sm:col-span-2">
						<span class="text-xs font-medium text-muted-foreground">Permissions</span>
						<Input bind:value={editingUser.privileges} disabled={!credentialsReady} placeholder="SELECT, INSERT, UPDATE or ALL PRIVILEGES" title="Comma-separated permissions to grant." />
					</label>
				</div>
				<Button onclick={onSave} disabled={busy || !credentialsReady} title="Save edits for this MariaDB user">
					<SaveIcon />
					Save Changes
				</Button>
			</div>
		{/if}
	</Card.Content>
</Card.Root>
