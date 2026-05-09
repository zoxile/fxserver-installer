<script lang="ts">
	import UserPlusIcon from "@lucide/svelte/icons/user-plus";
	import UserRoundCogIcon from "@lucide/svelte/icons/user-round-cog";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import * as ToggleGroup from "$lib/components/ui/toggle-group/index.js";

	type UserForm = {
		username: string;
		password: string;
		host: string;
		database: string;
		privileges: string;
	};

	type Props = {
		busy: boolean;
		credentialsReady: boolean;
		userConfig: UserForm;
		onSave: () => void;
	};

	let { busy, credentialsReady, userConfig = $bindable(), onSave }: Props = $props();
	const commonPrivileges = ["SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "ALTER", "INDEX", "DROP"];
	const allPrivileges = "ALL PRIVILEGES";

	function selectedPrivileges() {
		return userConfig.privileges
			.split(",")
			.map((privilege) => privilege.trim().toUpperCase())
			.filter(Boolean);
	}

	function toggleGroupPrivileges() {
		return selectedPrivileges().includes(allPrivileges) ? commonPrivileges : selectedPrivileges();
	}

	function setPrivileges(privileges: string[]) {
		if (privileges.includes(allPrivileges)) {
			userConfig.privileges = allPrivileges;
			return;
		}

		userConfig.privileges = privileges.join(", ");
	}

	function updatePrivileges(next: string[]) {
		if (next.includes(allPrivileges)) {
			setPrivileges([allPrivileges]);
			return;
		}

		setPrivileges(next);
	}

	function useAllPrivileges() {
		setPrivileges([allPrivileges]);
	}
</script>

<Card.Root class="h-full min-h-[34rem] rounded-md border-border bg-card shadow-sm">
	<Card.Header class="border-b border-border pb-4">
		<div class="flex items-center gap-3">
			<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
				<UserRoundCogIcon class="size-5" />
			</div>
			<div>
				<Card.Title>Add Database User</Card.Title>
				<Card.Description>Create a new account and optionally grant database permissions.</Card.Description>
			</div>
		</div>
	</Card.Header>

	<Card.Content class="space-y-5">
		<div class="grid gap-4 sm:grid-cols-2">
			<label class="grid gap-2">
				<span class="text-xs font-medium text-muted-foreground">Username</span>
				<Input bind:value={userConfig.username} disabled={!credentialsReady} placeholder="fxserver" title="Database username to create or update." />
			</label>
			<label class="grid gap-2">
				<span class="text-xs font-medium text-muted-foreground">Password</span>
				<Input type="password" bind:value={userConfig.password} disabled={!credentialsReady} placeholder="User password" title="Password for this database user." />
			</label>
			<label class="grid gap-2">
				<span class="text-xs font-medium text-muted-foreground">Host</span>
				<Input bind:value={userConfig.host} disabled={!credentialsReady} placeholder="localhost or %" title="Host pattern for this database account." />
			</label>
			<label class="grid gap-2">
				<span class="text-xs font-medium text-muted-foreground">Database</span>
				<Input bind:value={userConfig.database} disabled={!credentialsReady} placeholder="fxserver" title="Database to grant permissions on." />
			</label>
			<label class="grid gap-2 sm:col-span-2">
				<span class="text-xs font-medium text-muted-foreground">Permissions</span>
				<Input bind:value={userConfig.privileges} disabled={!credentialsReady} placeholder="SELECT, INSERT, UPDATE or ALL PRIVILEGES" title="Comma-separated privileges to grant." />
			</label>
		</div>

		<div class="space-y-3 rounded-sm border border-border bg-background/60 p-3">
			<div class="flex items-center justify-between gap-3">
				<div>
					<p class="text-xs font-medium text-muted-foreground">Quick permissions</p>
					<p class="mt-1 text-xs text-muted-foreground">Toggle common grants, or choose all privileges.</p>
				</div>
				<Button variant="outline" size="xs" onclick={useAllPrivileges} disabled={!credentialsReady} title="Grant all privileges on the selected database">
					All Privileges
				</Button>
			</div>
			<ToggleGroup.Root
				type="multiple"
				value={toggleGroupPrivileges()}
				onValueChange={updatePrivileges}
				disabled={!credentialsReady}
				class="grid grid-cols-2 gap-2 sm:grid-cols-4"
				aria-label="Database user permissions"
			>
				{#each commonPrivileges as privilege}
					<ToggleGroup.Item
						value={privilege}
						title={`Toggle ${privilege} permission`}
						class="w-full"
					>
						{privilege}
					</ToggleGroup.Item>
				{/each}
			</ToggleGroup.Root>
		</div>

		<div class="flex flex-wrap gap-2">
			<Button onclick={onSave} disabled={busy || !credentialsReady} title={credentialsReady ? "Create or update this MariaDB user" : "Apply valid admin credentials before adding users"}>
				<UserPlusIcon />
				Add User
			</Button>
		</div>
	</Card.Content>
</Card.Root>
