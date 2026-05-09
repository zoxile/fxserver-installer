<script lang="ts">
	import ArchiveIcon from "@lucide/svelte/icons/archive";
	import DatabaseIcon from "@lucide/svelte/icons/database";
	import ServerCogIcon from "@lucide/svelte/icons/server-cog";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import type { PageId } from "$lib/navigation";

	type Props = {
		onNavigate: (page: PageId) => void;
	};

	let { onNavigate }: Props = $props();

	const modules = [
		{
			title: "MariaDB",
			description: "Install, detect, control services, manage users, and run SQL.",
			icon: DatabaseIcon,
			page: "mariadb" as PageId,
		},
		{
			title: "Artifacts",
			description: "Prepare artifact download and inspection workflows.",
			icon: ArchiveIcon,
			page: "artifact-install" as PageId,
		},
		{
			title: "FXServer",
			description: "Server setup steps will live here as the installer grows.",
			icon: ServerCogIcon,
			page: "server" as PageId,
		},
	];
</script>

<section class="space-y-6">
	<div>
		<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Home</p>
		<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Home</h1>
		<p class="mt-2 max-w-2xl text-sm text-muted-foreground">
			A focused control surface for preparing a FiveM server without jumping between installers, folders, and command prompts.
		</p>
	</div>

	<div class="grid gap-4 lg:grid-cols-3">
		{#each modules as module}
			{@const Icon = module.icon}
			<Card.Root class="rounded-md border-border bg-card shadow-sm">
				<Card.Header class="border-b border-border pb-4">
					<div class="flex items-center gap-3">
						<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
							<Icon class="size-4" />
						</div>
						<div>
							<Card.Title>{module.title}</Card.Title>
							<Card.Description>{module.description}</Card.Description>
						</div>
					</div>
				</Card.Header>
				<Card.Content>
					<Button variant="outline" onclick={() => onNavigate(module.page)} title={`Open ${module.title}`}>
						Open
					</Button>
				</Card.Content>
			</Card.Root>
		{/each}
	</div>
</section>
