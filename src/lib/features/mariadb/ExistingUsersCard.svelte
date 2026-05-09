<script lang="ts">
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import SaveIcon from "@lucide/svelte/icons/save";
	import ShieldIcon from "@lucide/svelte/icons/shield";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import UsersIcon from "@lucide/svelte/icons/users";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import type { MariaDBUser } from "$lib/modules/mariadb";

	type EditableUser = {
		username: string;
		host: string;
		password: string;
		database: string;
		privileges: string;
	};

	type Props = {
		busy: boolean;
		users: MariaDBUser[];
		editingUser: EditableUser | null;
		onRefresh: () => void;
		onEdit: (user: MariaDBUser) => void;
		onSave: () => void;
		onDelete: (user: MariaDBUser) => void;
	};

	let {
		busy,
		users,
		editingUser = $bindable(),
		onRefresh,
		onEdit,
		onSave,
		onDelete,
	}: Props = $props();
</script>

<Card.Root class="h-full rounded-md border-border bg-card shadow-sm">
	<Card.Header class="border-b border-border pb-4">
		<div class="flex items-start justify-between gap-3">
			<div class="flex items-center gap-3">
				<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
					<UsersIcon class="size-5" />
				</div>
				<div>
					<Card.Title>Existing Users</Card.Title>
					<Card.Description>Review, edit, and remove accounts already present in MariaDB.</Card.Description>
				</div>
			</div>
			<Button variant="outline" size="icon" onclick={onRefresh} disabled={busy} title="Refresh existing MariaDB users">
				<RefreshCwIcon class={busy ? "animate-spin" : undefined} />
			</Button>
		</div>
	</Card.Header>

	<Card.Content class="space-y-4">
		{#if users.length === 0}
			<div class="rounded-sm border border-dashed border-border bg-background/60 p-4 text-sm text-muted-foreground">
				No users loaded. Refresh after entering valid connection credentials.
			</div>
		{:else}
			<div class="max-h-72 space-y-2 overflow-y-auto pr-1">
				{#each users as user}
					<div class="flex items-center justify-between gap-3 rounded-sm border border-border bg-background/70 px-3 py-2">
						<button class="min-w-0 flex-1 text-left" onclick={() => onEdit(user)} title={`Edit ${user.username}@${user.host}`}>
							<p class="truncate text-sm font-medium text-foreground">{user.username || "(anonymous)"}@{user.host}</p>
							<p class="mt-1 flex items-center gap-2 truncate text-xs text-muted-foreground">
								<ShieldIcon class="size-3.5" />
								{user.plugin || "plugin unknown"} · locked {user.locked || "unknown"}
							</p>
						</button>
						<Button variant="destructive" size="icon" onclick={() => onDelete(user)} disabled={busy} title={`Delete ${user.username}@${user.host}`}>
							<Trash2Icon />
						</Button>
					</div>
				{/each}
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
						<Input type="password" bind:value={editingUser.password} placeholder="New password or blank" title="New password for this MariaDB user." />
					</label>
					<label class="grid gap-2">
						<span class="text-xs font-medium text-muted-foreground">Database</span>
						<Input bind:value={editingUser.database} placeholder="fxserver" title="Database to grant permissions on." />
					</label>
					<label class="grid gap-2 sm:col-span-2">
						<span class="text-xs font-medium text-muted-foreground">Permissions</span>
						<Input bind:value={editingUser.privileges} placeholder="SELECT, INSERT, UPDATE or ALL PRIVILEGES" title="Comma-separated permissions to grant." />
					</label>
				</div>
				<Button onclick={onSave} disabled={busy} title="Save edits for this MariaDB user">
					<SaveIcon />
					Save Changes
				</Button>
			</div>
		{/if}
	</Card.Content>
</Card.Root>
